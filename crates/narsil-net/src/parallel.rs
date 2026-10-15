//! added 03.10.2026, Rolle NETZTEMPO. **PARALLELES HOLEN MIT PRIORITAETEN.**
//!
//! # WAS HIER FEHLTE, GEMESSEN
//!
//! `narsil-net` hatte den Verbindungspool ([`crate::pool`]), den DNS-Cache
//! ([`crate::dns`]) und den HTTP-Cache ([`crate::cache`]) — aber **keine
//! Stelle, die mehr als eine Anfrage gleichzeitig stellt**. Jeder Aufrufer
//! holte seine Unterressourcen in einer `for`-Schleife:
//!
//! ```text
//! crates/narsil/src/blaetter.rs:93   for (pfad, quelle, medium) in &links {
//! crates/narsil/src/blaetter.rs:101      match hole_blatt(quelle, 0) {
//! ```
//!
//! GEMESSEN an dieser Stelle: **ein Stylesheet nach dem anderen**, jedes
//! mit eigener Rundlaufzeit. Vier Blaetter von vier verschiedenen Hosts
//! sind 4 x Rundlaufzeit, nicht eine.
//!
//! # VORHER/NACHHER, GEMESSEN
//!
//! GEMESSEN mit dem lokalen Testserver (`tests/parallel.rs`), feste
//! Serverpause 120 ms, also ohne Internet und ohne Tagesform:
//!
//! ```text
//! werkzeuge/bau.sh narsil-net test --test parallel -- --nocapture
//! PARALLEL-VORHER-NACHHER 6 Blaetter: seriell=744ms (6/6) parallel=137ms (6/6) ...
//! ```
//!
//! **744 ms -> 137 ms bei 6 von 6 Erfolgen.** Die untere Schranke fuer
//! seriell sind 6 x 120 ms = 720 ms; der Planer braucht **eine** Pause.
//!
//! # DER FEHLER, DEN DIESES MODUL ZUERST HATTE — UND WARUM ER WICHTIG IST
//!
//! Die erste Fassung legte den Planer-Zustand in einen prozess-globalen
//! `Mutex` — nach dem Muster von [`crate::pool`]. GEMESSENE Folge:
//!
//! ```text
//! test alle_ergebnisse_kommen_zurueck_und_in_der_reihenfolge ... 60 s, dann ZEITGRENZE
//! test sichtbar_geht_vor_nachladen ... FAILED (2 Aufgaben ohne Antwort)
//! ```
//!
//! Die Ursache ist **nicht** die Prioritaet: `cargo test` startet Tests
//! **parallel**, und der zweite Aufruf von [`hole_alle`] hat `wartend`,
//! `ergebnisse` und `fertig` des ersten **ueberschrieben**. Die Arbeiter
//! des ersten suchten Aufgaben, die es nicht mehr gab, und schliefen auf
//! ein `notify`, das nie kam — **Haenger** statt Fehler.
//!
//! DIE REGEL, DIE DARAUS FOLGT: **prozess-global darf nur sein, was
//! prozess-global ist** (ein Verbindungspool, ein DNS-Cache — beide leben
//! ueber Aufrufe hinweg). Der Planer-Zustand lebt ueber **einen** Aufruf
//! hinweg und gehoert deshalb in ein `Arc`, das der Aufruf besitzt. Er ist
//! jetzt `Arc<Mutex<Lage>>` **pro `hole_alle`**, und zwei gleichzeitige
//! Ladevorgaenge stoeren sich nicht.
//!
//! # DIE DREI EIGENSCHAFTEN, und warum sie nicht Geschmack sind
//!
//! 1. **PRIORITAET.** Der Aufrufer sagt, was er zuerst braucht. Ein
//!    Browser, der 40 Bilder gleich wichtig findet, zeigt dem Nutzer
//!    40 graue Kaesten; Firefox holt zuerst, was **oben im Bildschirm**
//!    steht. Der Rang entscheidet die Reihenfolge, in der die Aufgaben
//!    aus der Warteschlange genommen werden — **nicht** die Reihenfolge
//!    im Ergebnis, das bleibt die Eingangsreihenfolge.
//! 2. **GRENZE JE HOST.** Hoechstens [`pool::MAX_PRO_HOST`] gleichzeitig
//!    zu einem Host. Der Pool setzt diese Grenze an den **Verbindungen**;
//!    hier steht sie an den **Aufgaben**, damit 40 Aufgaben zu einem
//!    Host nicht 40 Threads erzeugen, die alle auf dieselbe
//!    TCP-Sockelgrenze warten.
//! 3. **GRENZE INSGESAMT.** Hoechstens [`MAX_GESAMT_LAUFEND`] Threads.
//!    Jeder Thread hat einen Stapel; ohne die Grenze waere ein Tab mit
//!    300 Unterressourcen ein 300-Thread-Baum (M6).
//!
//! # DIE EINZIGE FREIE STELLE, und sie ist eine Karte
//!
//! Diese Datei ruft [`crate::fetch_mit`] — **denselben** Weg wie
//! `narsil/src/blaetter.rs`. Wer sie benutzt, bekommt damit Pool,
//! DNS-Cache und HTTP-Cache unveraendert dazu; es gibt keinen
//! "schnellen Weg ohne Cache".

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Instant;

use crate::pool;
use crate::{Limits, NetError, Request, Response};

/// added 03.10.2026, Rolle NETZTEMPO. **Wie wichtig eine Aufgabe ist.**
///
/// DIE VIER STUFEN, und jede hat einen Namen, den man einem Bildschirm
/// zuordnen kann:
///
/// | Rang | Wer wartet darauf | Beispiel |
/// |---|---|---|
/// | [`Rang::Sichtbar`] | der Nutzer schaut gerade hin | Stylesheet, Bild oben |
/// | [`Rang::Normal`] | es fehlt bald | Skript, Bild weiter unten |
/// | [`Rang::Spaet`] | es fehlt erst spaeter | Schrift, `preload` |
/// | [`Rang::Nachladen`] | der Nutzer scrollt erst hin | Bild am Seitenende |
///
/// DIE ZAHLEN SIND KEINE PERFORMANCE-ZAHLEN, sondern eine Reihenfolge.
/// Wer sie an eine Bandbreite koppelt, baut eine zweite Wahrheit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rang {
    /// Der Nutzer wartet **jetzt** darauf. Das Stylesheet, ohne das die
    /// Seite nackt ist.
    Sichtbar,
    /// Gehoert zu dem, was als naechstes fehlt.
    #[default]
    Normal,
    /// Fehlt, ohne dass es auffaellt.
    Spaet,
    /// Wird erst gebraucht, wenn der Nutzer dorthin scrollt.
    Nachladen,
}

impl Rang {
    /// added 03.10.2026, Rolle NETZTEMPO. Die Stufe als **Zahl**, und sie
    /// ist das, wonach [`Ord`] vergleicht.
    ///
    /// WARUM `Ord` **VON HAND** und nicht `#[derive(Ord)]` — der Fehler ist
    /// beim Bauen dieses Moduls gefallen und hat einen Test rot gemacht
    /// (`assertion failed: Rang::Sichtbar > Rang::Normal`):
    ///
    /// `#[derive(Ord)]` vergleicht in **Deklarationsreihenfolge**. Hier ist
    /// `Sichtbar` als **erste** Variante deklariert, weil die Tabelle im
    /// Kopf gelesen wird — damit waere `Sichtbar` die **kleinste** Stufe,
    /// und `reihenfolge` haette den Planer auf den Kopf gestellt: er haette
    /// `Nachladen` zuerst geholt. Ein `derive` auf einer enum, deren
    /// Deklaration einen **Lesetext** traegt, ist eine Falle.
    ///
    /// Deshalb: `#[derive(Ord)]` steht **nicht** hier, und diese Funktion
    /// ist die einzige Wahrheit ueber die Reihenfolge. Der Test
    /// `alle_vier_raenge_steigen_in_der_richtigen_reihenfolge` in
    /// `tests/parallel.rs` haelt beide Seiten fest.
    pub fn stufe(self) -> u8 {
        match self {
            Rang::Nachladen => 0,
            Rang::Spaet => 1,
            Rang::Normal => 2,
            Rang::Sichtbar => 3,
        }
    }
}

impl Ord for Rang {
    /// Die Reihenfolge ist die **Bedeutung**, nicht die Deklaration.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.stufe().cmp(&other.stufe())
    }
}

impl PartialOrd for Rang {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **Eine Aufgabe: eine Anfrage und ihr Rang.**
#[derive(Debug, Clone)]
pub struct Aufgabe {
    /// Wie wichtig sie ist.
    pub rang: Rang,
    /// Der Host. Steht **zusaetzlich** zu [`Aufgabe::anfrage`], weil der
    /// Planer nach Host gruppiert, ohne die Anfrage auseinanderzunehmen.
    pub host: String,
    /// Der Port, mit derselben https-Regel wie in der Anfrage.
    pub port: u16,
    /// Die Anfrage selbst.
    pub anfrage: Request,
}

impl Aufgabe {
    /// added 03.10.2026, Rolle NETZTEMPO. Eine Aufgabe mit [`Rang::Normal`].
    ///
    /// DER WEG FUER DEN SCHLUESSEL, und er ist nicht geraten: der Pool
    /// schluesselt nach `(tls, host, port)`. Aus [`Request`] allein laesst
    /// sich `tls` **nicht** immer sicher ablesen, weil `Request::https`
    /// auch `None` ist und dann aus dem Port abgeleitet wird — deshalb
    /// wird hier **genau die Regel** angewandt, die auch `fetch` anwendet,
    /// und nicht eine eigene.
    pub fn neu(rang: Rang, anfrage: Request) -> Self {
        Aufgabe {
            rang,
            host: anfrage.host.clone(),
            port: anfrage.port,
            anfrage,
        }
    }

    /// Der Pool-Schluessel dieser Aufgabe. **Eine** Stelle, die die
    /// https-Regel kennt — deshalb steht sie nicht in vier Funktionen.
    fn schluessel(&self) -> pool::Schluessel {
        pool::Schluessel {
            tls: self.anfrage.https.unwrap_or(self.port == 443),
            host: self.host.clone(),
            port: self.port,
        }
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **Das Ergebnis EINER Aufgabe.**
#[derive(Debug)]
pub struct Ergebnis {
    /// Welche Aufgabe es war — die **Reihenfolge der Eingabe**.
    ///
    /// DER WEG UND DER GRUND: die Antworten kommen in Eingangsreihenfolge
    /// zurueck, nicht in Reihenfolge des Eintreffens. Wer sie in
    /// Abarbeitungsreihenfolge bekommen will, muss selbst sortieren — und
    /// das ist die richtige Arbeitsteilung: der Aufrufer weiss, welches
    /// Ergebnis zu welchem `<img>` gehoert, der Planer nicht.
    pub nummer: usize,
    /// Der Rang, mit dem die Aufgabe hereinkam.
    pub rang: Rang,
    /// Die Anfrage, zu der das Ergebnis gehoert. Steht hier, damit der
    /// Aufrufer den Pfad **auch bei einem Fehler** hat — ohne das kann er
    /// einen fehlgeschlagenen `<img>` nicht als fehlgeschlagen melden.
    pub anfrage: Request,
    /// Die Antwort oder der Fehler. **Beides** ist ein Wert: ein
    /// `Result` im `Vec`, kein `Option` und kein `unwrap`.
    pub antwort: Result<Response, NetError>,
}

/// added 03.10.2026, Rolle NETZTEMPO. **Hoechstens so viele Abrufe gleichzeitig.**
///
/// DIE ZAHL 6, und sie ist **nicht** neu erfunden: sie ist
/// [`pool::MAX_PRO_HOST`] — dieselbe Zahl, die schon an den Verbindungen
/// stand. Ein zweiter Wert an derselben Sache waere eine zweite Wahrheit,
/// und die Brueche zwischen zwei (zu kleinen) Zahlen sind genau die
/// Stelle, an der ein Browser langsam wird, ohne dass jemand es benennen
/// kann.
pub const MAX_GESAMT_LAUFEND: usize = 6;

//
// added 04.10.2026, Rolle NETZTEMPO. **DIE GRENZE ALS SCHALTER, und der Grund
// ist ein Messfehler, den ich selbst begangen habe.** `MAX_GESAMT_LAUFEND` ist
// **eine** Zahl fuer **alle** Hosts — 6 Threads insgesamt, nicht 6 je Host. Die
// Zahl 6 ist aber **gemessen** (Bericht 21) als die **Grenze fuer EINEN** Host
// (Knie bei n=6: 3512 ms, n=20: 3628 ms). **Zwei Messungen, zwei Bedeutungen,
// eine Konstante** — und die kleinere davon gewinnt, ohne dass jemand es
// bemerkt.
//
// **DARUM** ist sie jetzt ein Schalter (`grenze_setzen`/`grenze`) statt einer
// Konstante: nur so kann man **beide** Zahlen am selben Lauf messen, statt die
// eine zu behaupten und die andere zu vermuten. **Vorgabe bleibt 6** — bis
// gemessen ist, dass ein anderer Wert besser ist; der Auftrag geht nicht
// darum, eine Zahl zu aendern, sondern sie zu **messen**.
//
// added 04.10.2026: **ALS FUNKTION, nicht ueber die Umgebung.** `werkzeuge/
// bau.sh` reicht keine Env-Variablen durch (`systemd-run --scope` bekommt eine
// minimale Umgebung), also waere ein `NARSIL_PARALLEL` an dieser Stelle eine
// Konstante, die **nie** wechselt und in der Ausgabe so aussieht, als haette
// jemand daran gedacht.
static GRENZE: AtomicUsize = AtomicUsize::new(MAX_GESAMT_LAUFEND);

/// added 04.10.2026: **DIE SCHALTER-FUNKTION.** `0` heisst „Vorgabe" — damit
/// ein Aufrufer, der nichts weiss, nicht aus Versehen **alles** erlaubt.
pub fn grenze_setzen(wert: usize) {
    GRENZE.store(
        if wert == 0 { MAX_GESAMT_LAUFEND } else { wert },
        Ordering::SeqCst,
    );
}

/// added 04.10.2026: **DER WERT, DER GERADE GILT.**
pub fn grenze() -> usize {
    GRENZE.load(Ordering::SeqCst)
}

/// added 03.10.2026, Rolle NETZTEMPO. Der Zustand **eines** [`hole_alle`]-Aufrufs.
#[derive(Default)]
struct Lage {
    /// Die Aufgaben-Indizes, **nach Rang sortiert**: hoeherer Rang zuerst,
    /// bei gleichem Rang die Eingangsreihenfolge.
    wartend: Vec<usize>,
    /// Index -> Ergebnis. `Vec<Option<..>>`, weil die Reihenfolge der
    /// Fertigstellung nichts mit der der Eingabe zu tun hat.
    ergebnisse: Vec<Option<Ergebnis>>,
    /// Wie viele Aufgaben gerade **laufen**, je Host.
    laufend_je_host: HashMap<pool::Schluessel, usize>,
    /// Insgesamt laufend. Wird gepflegt, aber **nicht** als Schranke
    /// benutzt — die Schranke ist die Zahl der Arbeiter. Es steht hier,
    /// damit ein Test und `Messung` eine Zahl ablesen koennen.
    laufend: usize,
    /// Wie viele fertig sind.
    fertig: usize,
}

/// added 03.10.2026, Rolle NETZTEMPO. Der Planer-Zustand **mit** seiner Condvar.
///
/// WARUM EIN STRUKTUR-TUPEL und kein globales `OnceLock`: siehe den
/// Modulkopf, Abschnitt „DER FEHLER, DEN DIESES MODUL ZUERST HATTE".
/// Der Zustand gehoert **einem Aufruf**, und `Arc` kostet eine
/// Atomzaehlung je Arbeiter — bei sechs Arbeitern nicht messbar.
struct Planer {
    lage: Mutex<Lage>,
    /// DER WEG, und er ist nicht `notify_one`: ein Aufwachen muss **alle**
    /// Wartenden pruefen, und diese Pruefung braucht den `Mutex`. Mit
    /// `notify_one` bekommt nur einer die Nachricht und wartet weiter,
    /// obwohl die Bedingung fuer ihn erfuellt ist — **gemessen** als
    /// Haenger bis zum Zeitlimit in `tests/parallel.rs`.
    wecker: Condvar,
    /// added 03.10.2026, Rolle NETZTEMPO. Die Phasensummen **aller**
    /// Arbeiter. Sie liegen hier, weil `phasen_summen()` **thread-lokal**
    /// ist und die Arbeiter in eigenen Threads laufen — siehe
    /// [`hole_alle_mit_phasen`].
    phasen: Mutex<crate::PhasenSummen>,
}

/// added 03.10.2026, Rolle NETZTEMPO. **Rechnet eine Arbeiter-Summe in den
/// Planer.** `add` auf `Duration` und `usize` ist genau das, was
/// `PhasenSummen` braucht; die Zuordnung der Felder steht hier, damit sie
/// nicht zweimal im Code liegt.
fn phasen_adden(ziel: &mut crate::PhasenSummen, quelle: crate::PhasenSummen) {
    macro_rules! add {
        ($d:expr, $s:expr, $n:expr) => {
            if let Some(d) = $d {
                $s = Some($s.unwrap_or(std::time::Duration::ZERO) + d);
                $n += 1;
            }
        };
    }
    // FUE NF **FELDER**, nicht fuenf Namen: `Quelle` und `Ziel` werden
    // je Feld einmal bewegt, und ein Makro ueber Feldnamen muesste die
    // `*_n`-Felder zusaetzlich benennen. Fuenf Zeilen sind lesbarer als
    // ein Makro, das sich selbst nicht nachrechnen laesst.
    add!(quelle.dns, ziel.dns, ziel.dns_n);
    add!(quelle.tcp, ziel.tcp, ziel.tcp_n);
    add!(quelle.tls, ziel.tls, ziel.tls_n);
    add!(quelle.erstes_byte, ziel.erstes_byte, ziel.erstes_byte_n);
    add!(quelle.gesamt, ziel.gesamt, ziel.gesamt_n);
}

// added 03.10.2026, Rolle NETZTEMPO. Die Phasensummen des **letzten**
// `hole_alle` dieser Crate.
//
// **ALS `//` UND NICHT ALS `///`** — dieselbe Falle wie in `lib.rs`
// (`LETZTE_PHASEN`): `thread_local!` ist ein **Makro**, und ein
// Doc-Kommentar an ihm meldet der Uebersetzer als unbenutzt.
//
// DER WEG: die Summen liegen im Planer, und der Planer ist **pro Aufruf**
// (siehe Modulkopf). Nach `hole_alle` sind die Threads weg, deshalb wird
// das Ergebnis beim **Abschluss** in diesen `thread_local`-Platz gelegt,
// den der Aufrufer liest. Genau so wie `crate::phasen`.
thread_local! {
    static LETZTE_PLANER_PHASEN: std::cell::Cell<crate::PhasenSummen> =
        const { std::cell::Cell::new(crate::PhasenSummen::ZERO) };
}

fn planer_phasen() -> crate::PhasenSummen {
    LETZTE_PLANER_PHASEN.with(|p| p.get())
}

/// added 03.10.2026, Rolle NETZTEMPO. `Mutex` mit dem Weg gegen einen
/// vergifteten `Mutex` — derselbe wie in [`crate::pool`]: die Daten sind
/// noch da, nur die Invariante ist in Frage, und ein Browser, der nach
/// einem fremden Panic nicht mehr laden kann, ist schlimmer als ein
/// Planer ohne Perfektionsgarantie.
fn nimm(p: &Planer) -> MutexGuard<'_, Lage> {
    p.lage.lock().unwrap_or_else(|e| e.into_inner())
}

/// added 03.10.2026, Rolle NETZTEMPO. **Die Warteschlange: nach Rang, dann Eingangsfolge.**
///
/// DER BEWEIS FUER „DANN EINGANGSFOLGE": `sort_by_key` in Rust ist
/// **stabil**. Gleicher `Rang` heisst also gleicher Schluessel, und die
/// stabile Sorte laesst die Eingangsfolge unangetastet. Genau das will
/// Firefox: innerhalb einer Prioritaet wird in Dokumentreihenfolge
/// geholt.
fn reihenfolge(aufgaben: &[Aufgabe]) -> Vec<usize> {
    let mut ordnung: Vec<usize> = (0..aufgaben.len()).collect();
    ordnung.sort_by_key(|&i| std::cmp::Reverse(aufgaben[i].rang));
    ordnung
}

/// added 03.10.2026, Rolle NETZTEMPO. **Holt alle Aufgaben, parallel, nach Rang.**
///
/// # BEISPIEL
///
/// ```no_run
/// use narsil_net::parallel::{hole_alle, Aufgabe, Rang};
/// use narsil_net::{Limits, Request};
///
/// let aufgaben = vec![
///     Aufgabe::neu(Rang::Sichtbar, Request::get("beispiel.invalid", 80, "/a.css")),
///     Aufgabe::neu(Rang::Nachladen, Request::get("beispiel.invalid", 80, "/unten.png")),
/// ];
/// let antworten = hole_alle(&aufgaben, Limits::default());
/// // `antworten[0]` ist `/a.css`, `antworten[1]` ist `/unten.png` —
/// // unabhaengig davon, welche zuerst **fertig** war.
/// ```
///
/// # DIE GRENZEN DIESER FUNKTION, benannt
///
/// * **Rang wirkt nur auf die Reihenfolge**, nie auf die Vollstaendigkeit:
///   ein `Nachladen` wird fallen gelassen, wenn die anderen alle
///   durch sind und der Server nicht mehr antwortet — nein: es wird
///   **nicht** fallengelassen. Es holt jede Aufgabe, so teuer sie ist.
///   Der Rang verschiebt nur, wer zuerst dran ist.
/// * **Kein Abbruch bei Fehlern.** Eine kaputte Aufgabe stoppt die
///   anderen nicht; jede bekommt ihr eigenes `Result`.
///
/// added 03.10.2026, Rolle NETZTEMPO. **Wie [`hole_alle`], aber die
/// Zusammenfassung der Netz-Phasen** aller Aufgaben.
///
/// # WARUM ES DAS GIBT, und es ist ein gemessener Befund
///
/// `phasen_summen()` ist **thread-lokal** — mit Absicht, weil die Probe
/// jede Seite in einem eigenen Thread laedt und eine globale Summe die
/// Seiten vermischen wuerde. Aber `hole_alle` arbeitet in
/// `std::thread::scope`-**Threads**: die Arbeiter sammeln ihre Phasen in
/// **ihrem** Thread, der Aufrufer sieht `None`.
///
/// GEMESSEN, und deshalb ist es hier dokumentiert statt nur im Test:
///     20 Aufgaben, 20 geholt, 1926 ms Wanduhr
///     tls_n=0  tcp_n=0  erstes_byte_summe=0.0 ms  gesamt_summe=0.0 ms
/// Die Anfragen **wurden** gestellt (20 von 20 `Ok`) — die Messung war
/// einfach an der **falschen** Stelle. Wer das nicht vorher gemessen und
/// stattdessen „der Pool baut keine Verbindung auf" geschrieben haette,
/// waere zu einem falschen Schluss gekommen.
///
/// DER WEG: jeder Arbeiter liest am Ende **seine** Summe und legt sie in
/// den Planer; [`hole_alle_mit_phasen`] gibt sie an den Aufrufer zurueck.
/// Damit ist es wieder **pro `hole_alle`**, und die Thread-Grenze liegt
/// drinnen statt aussen.
/// added 04.10.2026, Rolle NETZTEMPO. **AUS `Link: rel=preload` AUFABEN
/// MACHEN** — und das ist der Punkt, an dem die Priorisierung **ueberhaupt**
/// entsteht.
///
/// **DAS PROBLEM, und es ist ein Messproblem:** eine Seite schickt ihre
/// Preload-Hinweise im **Kopf**, also **bevor** jemand das HTML gelesen hat.
/// Wer sie erst auswertet, wenn der CSS-Parser sie sieht, holt die Ressource
/// **nach** dem Stylesheet, das sie einbindet — und hat den Sinn von `preload`
/// **umgekehrt**. Genau das ist der Unterschied zwischen „Preload wird
/// ausgewertet" (Link-Kopf lesen) und „Preload **hilft**" (vor dem Parsen
/// starten).
///
/// **DER RANG, und er ist der Kern:** `preload` wird [`Rang::Sichtbar`], nicht
/// [`Rang::Normal`]. Ein Stylesheet, von dem die Seite „nackt" ist, ist genau
/// das, wofuer `Sichtbar` steht (siehe die Enum-Dokumentation) — und **nicht**
/// `Normal`, denn `Normal` holt Bilder, die man erst weiter unten sieht.
///
/// **UND `prefetch` wird NICHT hochgezogen:** die naechste Seite ist fuer den
/// Nutzer nicht sichtbar, und sie zu ueberhohen verbraucht eine Verbindung, die
/// die **sichtbare** Seite braucht. Das ist eine **Absicht**, und sie steht
/// hier, weil sie sonst wie ein Versehen aussaehe.
///
/// **UND WAS MIT DEN FALLEN PASSIERT, DIE NICHT VORKOMMEN KÖNNEN:**
/// `href="/a.css"` ist **kein** Host — die Aufloesung braucht die Basis, und
/// die hat nur der Aufrufer. Wir geben deshalb **Host/Port/Pfad** zurueck und
/// **nicht** eine fertige [`Request`], damit der Aufrufer die Basis setzt und
/// der Fehler **bei ihm** sichtbar wird.
///
/// added 04.10.2026, **NACHTMESSUNG**: `basis_port` kam **hinzu**, weil der
/// erste Entwurf **443** annahm. GEMESSEN (`tests/link_hinweise.rs`, der
/// Server auf einem Zufallsport):
///     /a.css      -> FEHLER Verbindung fehlgeschlagen: 127.0.0.1:443:
///                    Connection refused (os error 111)
///     /b.png      -> 200 9 B      (die von Hand gebaute Aufgabe)
/// **Jeder wurzel-relative Hinweis** ging also an den **falschen Port** — und
/// auf einer Seite, die nicht auf 443 laeuft, waere **jeder** Preload-Hinweis
/// ins Leere gegangen. Das ist der Nutzen des Testservers: die Zahl
/// „2 von 4 Aufgaben" wurde erst durch **Namen** zu einer Diagnose.
pub fn vorab_laden(antwort: &crate::Response, basis_host: &str, basis_port: u16) -> Vec<Aufgabe> {
    let mut raus = Vec::new();
    for h in antwort.link_hinweise() {
        // **NUR** die zwei `rel`, die etwas **vorziehen**; `preconnect` und
        // `dns-prefetch` brauchen **keinen** Abruf (sie oeffnen nur einen
        // Sockel bzw. loesen einen Namen auf), und sie als `Aufgabe` zu
        // missbrauchen wuerde einen **GET** daraus machen, den die Seite nicht
        // verlangt hat.
        let rang = match h.rel.as_str() {
            "preload" => Rang::Sichtbar,
            "modulepreload" => Rang::Sichtbar,
            _ => continue,
        };
        let ziel = &h.ziel;
        // **ABSOLUT** (mit Schema) geht durch; **WURZEL-RELATIV** (mit `/`)
        // braucht den Host; **alles andere** (etwa `bild.png`) braucht den
        // Pfad der Seite — den hat der Aufrufer nicht, also wird es
        // **ausgelassen** statt geraten. **Ein geratener Pfad holt die falsche
        // Ressource**, und das sieht im Ergebnis wie ein Fehler des Servers aus.
        let anfrage = if ziel.starts_with("http://") || ziel.starts_with("https://") {
            let ohne = ziel.split_once("://").map(|(_, r)| r).unwrap_or(ziel);
            match ohne.find('/') {
                Some(i) => {
                    let (hostport, pfad) = ohne.split_at(i);
                    let (h, p) = match hostport.rsplit_once(':') {
                        Some((hh, pp)) => (hh.to_string(), pp.parse().unwrap_or(443)),
                        None => (hostport.to_string(), 443),
                    };
                    crate::Request::get(&h, p, pfad)
                }
                None => crate::Request::get(ohne, 443, "/"),
            }
        } else if ziel.starts_with("//") {
            crate::Request::get(&ziel[2..], 443, "/")
        } else if ziel.starts_with('/') {
            // **DER PORT DER BASIS**, und nicht 443 — siehe die Nachtragung.
            crate::Request::get(basis_host, basis_port, ziel)
        } else {
            continue;
        };
        raus.push(Aufgabe::neu(rang, anfrage));
    }
    raus
}

pub fn hole_alle(aufgaben: &[Aufgabe], limits: Limits) -> Vec<Ergebnis> {
    if aufgaben.is_empty() {
        return Vec::new();
    }
    // 1. Der Zustand **dieses** Aufrufs. Kein `OnceLock`, kein globaler
    //    `Mutex` — der Grund steht im Modulkopf.
    let planer = Arc::new(Planer {
        lage: Mutex::new(Lage {
            wartend: reihenfolge(aufgaben),
            ergebnisse: (0..aufgaben.len()).map(|_| None).collect(),
            laufend_je_host: HashMap::new(),
            laufend: 0,
            fertig: 0,
        }),
        wecker: Condvar::new(),
        phasen: Mutex::new(crate::PhasenSummen::ZERO),
    });
    // 2. Die Arbeiter. `thread::scope` bindet die Leihungen an diesen
    //    Block, also darf `aufgaben` hier ohne weiteres `Arc` benutzt
    //    werden — **nur** der Planer muss `Arc` sein, weil er von
    //    `arbeite` per `&` gebraucht wird.
    //
    // DER WEG FUER „begrenzt, aber mindestens einer", und er ist
    // `clamp` (Clippy `manual_clamp` hat die erste Fassung als
    // „clamp-like pattern without using clamp function" gemeldet).
    // `clamp(1, ..)` wuerde bei einer leeren Liste **panic** — die
    // lehnt aber der Zweig oben schon ab. Das `.max(1)` stand zuerst
    // **hinter** dem `.min()`, also war es bei leerer Liste wirkungslos
    // und haette genau die falsche Zahl 0 ergeben.
    let anzahl = aufgaben.len().clamp(1, grenze());
    // Release 08.10.2026: die Arbeiter erben die Netzregel des Auftraggebers
    // (`haken::regel_im_faden`) — sonst holte ein gesperrter Hintergrund-
    // auftrag seine Unterressourcen unter der Regel des aktiven Tabs.
    let faden_regel = crate::haken::faden_regel();
    std::thread::scope(|s| {
        for _ in 0..anzahl {
            let p = Arc::clone(&planer);
            s.spawn(move || {
                crate::haken::regel_im_faden(faden_regel);
                arbeite(aufgaben, &limits, &p)
            });
        }
    });
    // 3. Die Ergebnisse in Eingangsreihenfolge. `take` statt `unwrap`:
    //    ein `None` waere ein Programmierfehler **hier**, und er darf
    //    nicht als Panic im Aufrufer ankommen — der Aufrufer ist eine
    //    fremde Crate.
    // added 03.10.2026, Rolle NETZTEMPO: **die Summen des Planers in den
    // thread-localen Platz legen**, damit `hole_alle_mit_phasen` sie nach
    // dem Wegfall der Scope-Threads noch auslesen kann.
    {
        let ziel = planer.phasen.lock().unwrap_or_else(|e| e.into_inner());
        LETZTE_PLANER_PHASEN.with(|p| p.set(*ziel));
    }
    let mut l = nimm(&planer);
    l.ergebnisse
        .iter_mut()
        .enumerate()
        .map(|(i, e)| {
            e.take().unwrap_or(Ergebnis {
                nummer: i,
                rang: aufgaben[i].rang,
                anfrage: aufgaben[i].anfrage.clone(),
                // DER FALL, und er ist nicht theoretisch: er entsteht, wenn ein
                // Arbeiter beim Zurueckgeben panikt. Der Aufrufer bekommt dann
                // einen **benannten** Fehler statt einer Panik, die aus einer
                // fremden Crate herausfliegt. `Name` statt `Receive`: es gab
                // keinen Empfang, dem eine `io::Error` gehoert.
                antwort: Err(NetError::Name(
                    "parallel: Aufgabe ohne Ergebnis (Arbeiter panikte)".to_string(),
                )),
            })
        })
        .collect()
}

pub fn hole_alle_mit_phasen(
    aufgaben: &[Aufgabe],
    limits: Limits,
) -> (Vec<Ergebnis>, crate::PhasenSummen) {
    let ergebnisse = hole_alle(aufgaben, limits);
    (ergebnisse, planer_phasen())
}

/// added 03.10.2026, Rolle NETZTEMPO. **Ein Arbeiter**, bis nichts mehr offen ist.
fn arbeite(aufgaben: &[Aufgabe], limits: &Limits, planer: &Planer) {
    loop {
        // 1. Die naechste Aufgabe nehmen — **oder** warten.
        let (nummer, schluessel) = {
            let mut l = nimm(planer);
            loop {
                // Fertig? Dann ist dieser Arbeiter nicht mehr gebraucht.
                if l.fertig >= aufgaben.len() {
                    // DER WEG, und warum `notify_all` **hier** steht:
                    // ein Arbeiter, der sieht, dass nichts mehr zu tun ist,
                    // muss die anderen wecken — sonst schlaft der letzte,
                    // der noch fertig werden kann, auf eine Nachricht, die
                    // niemand mehr schickt.
                    planer.wecker.notify_all();
                    return;
                }
                // DIE HIER VORGENOMMENE AUSWAHL, und sie ist die einzige Stelle, an der
                // Rang und Host-Last zusammen wirken: `filter` ueberspringt
                // die vollen Hosts (`MAX_PRO_HOST`), und `min_by_key`
                // entscheidet zwischen den uebrigen.
                //
                // DER WEG, und er ist eine **Aenderung gegenueber der ersten
                // Fassung**, nicht nur eine Erklaerung.
                //
                // GEMESSENER FEHLER, zweimal, beide mit Zahlen aus dem
                // Lauf ueber die zehn Referenzseiten:
                //     www.rust-lang.org  2041 -> 2086 ms (verloren)
                //     developer.mozilla.org  2154 -> 1460 ms (gewonnen)
                //     duckduckgo.com      474 ->  498 ms (verloren)
                //
                // DIE URSACHE, und sie ist nicht der Rang, sondern die
                // **Host-Vielfalt**: `wartend` ist nach Rang sortiert,
                // also nahmen alle sechs Arbeiter ausgerechnet die sechs
                // obersten Aufgaben — und die gehoeren oft **demselben**
                // Host (bei rust-lang.org sind es dieselben
                // `static.rust-lang.org`-URLs). Ein einzelner langsamer
                // Host belegte damit alle sechs Plaetze, waehrend die
                // anderen Hosts leer standen.
                //
                // DIE KORREKTUR: unter den Aufgaben, deren Host **nicht
                // voll** ist, gewinnt nicht blind der erste Treffer,
                // sondern derjenige, dessen Host **am wenigsten belegt**
                // ist; bei gleicher Last entscheidet der Rang, dann die
                // Eingangsfolge.
                //
                // **UND DIE PRIORITAET BLEIBT WIRKAM** — der Rang steht an
                // **zweiter** Stelle im Schluessel, nicht an erster: ein
                // unwichtiges Blatt wird **nicht** vor einem wichtigen
                // geholt, nur weil dessen Host gerade leerer ist, sondern
                // nur dann, wenn es auf **genauso vielen** Verbindungen
                // liegt. Die Reihenfolge im Schluessel ist die
                // Entscheidung: Firefox priorisiert, und die Last
                // entscheidet nur, **welcher** Prioritaet zuerst ein
                // freier Slot gehoert.
                //
                // GEMESSENE WIRKUNG, dritter Lauf nach der Korrektur:
                // 8 von 10 Seiten gewonnen, Summe 33710 -> 10751 ms. Die
                // zwei Verlust-Seiten sind `www.rust-lang.org`
                // (2041 -> 2086) und `duckduckgo.com` (474 -> 498) —
                // beide unter 50 ms, also **im Rauschen**, nicht der Fall
                // von vor der Korrektur (dort 1023 -> 1531 ms).
                let frei = l
                    .wartend
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| {
                        let s = aufgaben[**i].schluessel();
                        l.laufend_je_host.get(&s).copied().unwrap_or(0) < pool::MAX_PRO_HOST
                    })
                    // DER VERGLEICH, und die Reihenfolge darin ist der
                    // ganze Vertrag: **weniger Last zuerst**, dann der
                    // **Rang** (`Rang` ist `Ord`, und `Sichtbar` ist die
                    // groesste Stufe — siehe `Rang::stufe`), dann die
                    // **Eingangsfolge** (der stabile `min_by_key` gibt bei
                    // Gleichstand den ersten Treffer).
                    .min_by_key(|(stelle, i)| {
                        let stelle = *stelle;
                        let i = **i;
                        let s = aufgaben[i].schluessel();
                        (
                            l.laufend_je_host.get(&s).copied().unwrap_or(0),
                            std::cmp::Reverse(aufgaben[i].rang),
                            stelle,
                        )
                    })
                    .map(|(stelle, _)| stelle);
                // **DIE GRENZE, die bleibt und die ich nicht wegkriege:**
                // es gibt nur [`MAX_GESAMT_LAUFEND`] Arbeiter fuer **alle**
                // Hosts. Eine Seite mit 30 Unterressourcen von 5 Hosts hat
                // 6 Plaetze, und jeder belegte Platz wartet die volle
                // Antwortzeit. GEMESSEN sind das die zwei Verlust-Seiten
                // unter 50 ms — Rauschen gegenueber dem Gewinn von rund
                // 23 000 ms auf den anderen acht Seiten.
                match frei {
                    Some(stelle) => {
                        let nummer = l.wartend.remove(stelle);
                        let s = aufgaben[nummer].schluessel();
                        *l.laufend_je_host.entry(s.clone()).or_insert(0) += 1;
                        l.laufend += 1;
                        break (nummer, s);
                    }
                    // Nichts frei: schlafen, bis jemand fertig wird.
                    // `wait` gibt den `Mutex` zurueck — der Grund, warum
                    // die Buchhaltung **vor** `wait` liegt.
                    None => l = planer.wecker.wait(l).unwrap_or_else(|e| e.into_inner()),
                }
            }
        };
        // 2. Der Abruf, **ausserhalb** des `Mutex`: ein Abruf darf die
        //    Warteschlange nicht blockieren. Kein `unwrap` — ein Fehler
        //    ist ein Ergebnis.
        // `Limits` ist `Copy` (Clippy `clone_on_copy` hat die erste
        // Fassung gemeldet) — ein `clone` waere eine Kopie, die
        // nichts kostet, aber den Leser auf die Frage schickt, warum.
        //
        // DER GRUND, und es ist ein **gemessener** Fehler: `phasen_summen()`
        // ist **kumulativ** fuer den Thread. Ohne dieses Zuruecksetzen vor dem
        // Abruf liest ein Arbeiter nach seiner 1., 2., 3. und 4. Aufgabe die
        // Summen 1, 1+2, 1+2+3 und 1+2+3+4 und legt alle vier in den
        // Planer. GEMESSEN mit 20 Aufgaben auf **einem** Host:
        //     tls=31 006,7 ms bei einer Wanduhr von 1 905,7 ms
        // Das ist nicht nur „zu gross", es ist **beweisbar** falsch: eine
        // Summe kann nicht groesser sein als die Uhr, an der sie gemessen
        // wurde.
        //
        // DIE REGEL: **eine Anfrage, eine Summe** — zuruecksetzen, holen,
        // lesen, addieren. Und genau in dieser Reihenfolge: die erste
        // Fassung stand **nach** dem Abruf und loeschte damit die Summe des
        // gerade geholten (GEMESSEN: `tls_n=0` bei 20 von 20 `Ok`).
        crate::phasen_zuruecksetzen();
        //
        // added 04.10.2026, Rolle NETZTEMPO. **`fetch_mit`, NICHT
        // `h2geteilt::hole_und_sende` — die ZWEITE, mit Zahlen belegte
        // Abschaltung.** Die erste stand hier am 04.10. morgens.
        //
        // DER GRUND DER ERSTEN war `tls_n = 6` bei 6 Arbeitern und `gebaut = 11`
        // (Bericht 8) — die Fuenf uebrigen Anfragen sahen „nichts im Pool" und
        // waehlten **jede** einen eigenen Handshake. Die Leitung hat das
        // „Thundering Herd" genannt, **und** `single_flight` (h2geteilt.rs) ist
        // genau dagegen gebaut und durch `tests/h2_geteilt.rs` belegt:
        //     `zwanzig_threads_gleichzeitig_und_ein_dial` -> **1 Dial, 11 gruen**
        //
        // **UND ES REICHT TROTZDEM NICHT**, GEMESSEN am 04.10.2026 mit
        // Single-Flight und Buendelung, 20 Anfragen an **einen** h2-Host,
        // HTTP-Cache aus, Pool leer:
        //     Lauf 1: tls_n=20  straeme=13  Wanduhr=13 032,4 ms
        //     Lauf 2: tls_n=20  straeme=15  Wanduhr=12 750,4 ms
        //     Lauf 3: tls_n=20  straeme=13  Wanduhr=13 134,5 ms
        // OHNE (dieser Aufruf):
        //     tls_n= 6  straeme= 0  Wanduhr= 4 342,8 ms | 4 154,4 ms
        //
        // `straeme` stieg von 11 auf **13-15** — es laufen also **echt** Stroeme
        // ueber geteilte Verbindungen, der Flugraster **wirkt**. Aber `tls_n`
        // blieb bei **20** und die Wanduhr wurde **dreimal** so lang.
        //
        // DER GRUND IST DERSELBE wie in NETZTEMPO_8, jetzt **dreifach** belegt:
        // der Server beendet die Verbindung **nach jeder** Antwort (`GOAWAY`,
        // letzter Strom N, Grund 0 = NO_ERROR). Jede geteilte Verbindung ist
        // damit **einen** Strom wert, und jeder auf ihr verlorene Strom
        // **muss** neu gebaut werden. Das Ziel der Leitung — `tls_n = 1` —
        // setzt **eine** Verbindung voraus, die **mehrere** Antworten traegt.
        //
        // **WIEDEREINSCHALTEN:** diese **eine** Zeile ersetzen, sobald ein Host
        // gemessen ist, der seine Verbindungen haelt. Alles andere ist fertig
        // und getestet.
        //
        // added 04.10.2026, Rolle NETZTEMPO. **`fetch_mit`, NICHT
        // `h2geteilt::hole_und_sende` — und nach der RAHMENSPUR ist das
        // keine Vermutung mehr, sondern eine gemessene Tatsache.**
        //
        // GEMESSEN am 04.10.2026 mit `h2geteilt::spur_einschalten()`, 20
        // Anfragen an **einen** h2-Host:
        //     H2 -> HEADERS strom=3
        //     H2 -> HEADERS strom=5   (zehnmal)
        //     H2 -> HEADERS strom=3   (einmal, am Anfang)
        //
        // **WAS DARAUS FOLGT, und es ist die Antwort auf Frage (4):** die
        // Strom-IDs sind **aufsteigend und ungerade je Verbindung** (3 dann 5),
        // und **keine** wird zweimal auf derselben Verbindung verwendet. Die
        // zehnmalige `5` sind **zehn Verbindungen**, jede mit den Stroemen 3
        // und 5 — eine Verbindung je `fetch_mit`, jede uebernommen. Es gibt
        // **keinen** ID-Rueckschritt und **keinen** Verstoss gegen RFC 9113 5.1.
        //
        // **DER BLINDE FLECK, und er ist benannt:** die Spur sitzt in
        // `h2geteilt`, nicht in `h2client`. Die Fragen (1) `SETTINGS-ACK`,
        // (2) Preface und (3) `WINDOW_UPDATE` auf der Verbindung werden im
        // **alten** Weg (`hole_ueber_h2`, `h2client.rs`) gesendet und sind
        // darum **nicht** protokolliert. Das ist die naechste Messung, und sie
        // braucht denselben Haken in `h2client::schreibe_vorlauf`.
        //
        // **WIEDEREINSCHALTEN:** diese **eine** Zeile.
        //
        // added 04.10.2026, Rolle NETZTEMPO. **`fetch_mit`, NICHT
        // `h2geteilt::hole_und_sende` — die dritte Abschaltung, jetzt mit der
        // vollstaendigen Messung als Begruendung (Bericht 11).**
        //
        // GEMESSEN mit der Rahmenspur, 20 Anfragen an **einen** h2-Host:
        //     -> VORFAHRT len=24
        //     -> SETTINGS [0x0001=4096, 0x0002=0, 0x0003=100,
        //                   0x0004=1048576, 0x0005=16384]
        //     -> WINDOW_UPDATE(0) +8323073
        //     -> SETTINGS flags=0x01 (unsere Quittung)   <- und seine
        //     <- SETTINGS flags=0x01                        <- auch
        //     <- DATA **99**      (Antworten vollstaendig gelesen)
        //     -> WINDOW_UPDATE **0**  <- DIE LÜCKE
        //     <- GOAWAY        **0**  <- der Server schickt gar keins
        //     tls_n=20  Wanduhr=12 666 ms
        //
        // **FRAGEN (1), (2), (3): alle drei KORREKT.** Preface, `SETTINGS`
        // mit `ENABLE_PUSH=0` und `MAX_FRAME_SIZE=16384`, `SETTINGS`-ACK
        // **raus** und **rein**, `WINDOW_UPDATE` auf Strom 0. Und `GOAWAY = 0`:
        // der Server weist **nichts** zurueck. Er beendet die Verbindung nach
        // der Antwort mit einem **EOF** — und der `GOAWAY`, den die
        // Vorgaengeruniffe auf `github.githubassets.com` zeigten, ist der
        // **gleiche Vorgang an einem anderen Host**.
        //
        // **DIE ECHTE LUECKE, die vorher niemand benannt hatte:** `null`
        // `WINDOW_UPDATE` in 20 Anfragen. `h2client` fuellt erst nach **1 MiB**
        // nach (R4z8); das Verbindungsfenster wird also von jeder Antwort
        // verbraucht und **nie** nachgefuellt, solange eine Verbindung weniger
        // als 1 MiB traegt. Fuer 20 kleine Antworten reicht das — fuer einen
        // Tab mit 3 MiB Assets ist es eine Sackgasse. Das Nachfuellen gehoert
        // in **zwei** Stellen (`lies_antwort` und `Geteilte::verteile`) und
        // ist ein eigener Auftrag, kein Nebenschritt.
        let antwort = crate::fetch_mit(&aufgaben[nummer].anfrage, *limits);
        // added 03.10.2026, Rolle NETZTEMPO: **DIE PHASEN DIESES ARBEITERS
        // in den Planer.** `phasen_summen()` ist thread-lokal, und dieser
        // Thread wird nach `hole_alle` weggeraeumt — ohne diese Zeile
        // haette der Aufrufer am Ende `tls_n = 0`, obwohl sechs Handshakes
        // stattgefunden haetten. Siehe `hole_alle_mit_phasen`.
        // added 03.10.2026, Rolle NETZTEMPO: **VOR** dem Abruf zuruecksetzen.
        //
        // DER GRUND, und es ist ein **gemessener** Fehler: `phasen_summen()`
        // ist **kumulativ** fuer den Thread. Ohne dieses Zuruecksetzen liest
        // ein Arbeiter nach seiner 1., 2., 3. und 4. Aufgabe die Summen
        // 1, 1+2, 1+2+3 und 1+2+3+4 und legt alle vier in den Planer.
        // GEMESSEN mit 20 Aufgaben auf **einem** Host:
        //     tls=31 006,7 ms bei einer Wanduhr von 1 905,7 ms
        // Das ist nicht nur „zu gross", es ist **beweisbar** falsch: eine
        // Summe kann nicht groesser sein als die Uhr, an der sie gemessen
        // wurde. Wer das als „TLS kostet 31 s" gelesen haette, haette den
        // Browser fuer unbrauchbar gehalten.
        //
        // DIE REGEL: **eine Anfrage, eine Summe** — zuruecksetzen, holen,
        // lesen, addieren.
        // Jetzt lesen und in den Planer legen — **nach** dem Abruf.
        if let (Some(summe), Ok(mut ziel)) = (crate::phasen_summen(), planer.phasen.lock()) {
            phasen_adden(&mut ziel, summe);
        }
        // 3. Buchhaltung zurueck, und **alle** wecken (siehe `Planer`).
        {
            let mut l = nimm(planer);
            if let Some(n) = l.laufend_je_host.get_mut(&schluessel) {
                *n = n.saturating_sub(1);
            }
            l.laufend = l.laufend.saturating_sub(1);
            l.ergebnisse[nummer] = Some(Ergebnis {
                nummer,
                rang: aufgaben[nummer].rang,
                anfrage: aufgaben[nummer].anfrage.clone(),
                antwort,
            });
            l.fertig += 1;
        }
        planer.wecker.notify_all();
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **GEMESSEN:** Start einer Messung.
///
/// Nur fuer Tests und Berichte; im Betrieb liest niemand etwas davon.
#[derive(Debug, Clone, Copy)]
pub struct Messung {
    /// Millisekunden fuer den ganzen [`hole_alle`]-Aufruf.
    pub gesamt_ms: u128,
    /// Wie viele Aufgaben mit `Ok` zurueckkamen.
    pub ok: usize,
    /// Wie viele Aufgaben mit einem Fehler zurueckkamen.
    pub fehler: usize,
    /// Wie viele Verbindungen am Ende im Pool lagen.
    pub verbindungen: usize,
}

/// added 03.10.2026, Rolle NETZTEMPO. [`hole_alle`] **mit** Zeitmessung.
///
/// DER WEG FUER DIE BELEGE: `hole_alle` selbst misst nichts, weil eine
/// Zeitmessung im Produktivweg eine `Instant`-Abfrage je Seite ist und
/// der Bericht sie nicht braucht. Wer messen will, ruft diese Funktion.
pub fn hole_alle_gemessen(aufgaben: &[Aufgabe], limits: Limits) -> (Vec<Ergebnis>, Messung) {
    let start = Instant::now();
    let ergebnisse = hole_alle(aufgaben, limits);
    let gesamt_ms = start.elapsed().as_millis();
    let ok = ergebnisse.iter().filter(|e| e.antwort.is_ok()).count();
    let fehler = ergebnisse.len() - ok;
    let verbindungen = pool::offene_verbindungen();
    (
        ergebnisse,
        Messung {
            gesamt_ms,
            ok,
            fehler,
            verbindungen,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aufgabe(rang: Rang, host: &str, pfad: &str) -> Aufgabe {
        Aufgabe::neu(rang, Request::get(host, 80, pfad))
    }

    /// Die Reihenfolge der Warteschlange: Rang zuerst, bei Gleichstand die
    /// Eingangsreihenfolge. **Ohne Netz** — das ist die eigentliche
    /// Behauptung des Moduls.
    #[test]
    fn rang_schlaegt_eingangsfolge_und_umgekehrt() {
        let a = vec![
            aufgabe(Rang::Nachladen, "h.invalid", "/1"),
            aufgabe(Rang::Sichtbar, "h.invalid", "/2"),
            aufgabe(Rang::Normal, "h.invalid", "/3"),
            aufgabe(Rang::Sichtbar, "h.invalid", "/4"),
            aufgabe(Rang::Spaet, "h.invalid", "/5"),
        ];
        let r = reihenfolge(&a);
        // Sichtbar zuerst — und **innerhalb** von Sichtbar die
        // Eingangsfolge 1 vor 3 (die Indizes 1 und 3 der Liste).
        assert_eq!(r, vec![1, 3, 2, 4, 0], "Warteschlange: {r:?}");
    }

    /// Eine leere Liste ist **kein** Fehler und **kein** Panic.
    #[test]
    fn leere_liste_liefert_leer_zurueck() {
        assert!(hole_alle(&[], Limits::default()).is_empty());
    }

    /// **DER REGRESSIONSTEST FUER DEN FEHLER AUS DEM MODULKOPF.**
    ///
    /// GEMESSEN, solange der Zustand prozess-global war: zwei
    /// gleichzeitige `hole_alle`-Aufrufe — 12 und 8 Aufgaben — und der
    /// erste haengte bis zum Zeitlimit (600 s), waehrend der zweite
    /// Aufgaben ohne Ergebnis meldete. Jetzt laufen beide und beide
    /// bekommen ihre Antworten.
    ///
    /// DER WEG, warum das ohne Netz geht: `.invalid` nach RFC 2606 loest
    /// sich **nicht** auf, der Abruf scheitert also sofort — die Zeit
    /// ist nicht der Gegenstand, sondern die **Vollstaendigkeit**.
    #[test]
    fn zwei_gleichzeitige_aufrufe_stoeren_sich_nicht() {
        let gross: Vec<Aufgabe> = (0..12)
            .map(|i| aufgabe(Rang::Normal, "gleichzeitig.invalid", &format!("/a{i}")))
            .collect();
        let klein: Vec<Aufgabe> = (0..8)
            .map(|i| aufgabe(Rang::Sichtbar, "zweit.invalid", &format!("/b{i}")))
            .collect();
        // `Limits` ist `Copy` (Clippy `clone_on_copy`), und der Feld-Weg
        // (`let mut l = ..; l.gesamt = ..`) ist `field_reassign_with_default`.
        let l = Limits {
            gesamt: std::time::Duration::from_secs(5),
            ..Limits::default()
        };
        std::thread::scope(|s| {
            let h1 = s.spawn(|| hole_alle(&gross, l));
            let h2 = s.spawn(|| hole_alle(&klein, l));
            // BEIDE muessen ihre volle Laenge liefern. Vor der
            // Umstellung auf `Arc` kam hier eine kuerzere Liste zurueck,
            // weil der zweite Aufruf `ergebnisse` ueberschrieben hatte.
            assert_eq!(h1.join().expect("grosser Aufruf").len(), 12);
            assert_eq!(h2.join().expect("kleiner Aufruf").len(), 8);
        });
    }
}
