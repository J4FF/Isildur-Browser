//! Der HTTP-Cache von aussen: prozess-global, abschaltbar, begrenzt.
//!
//! added 30.09.2026 fuer Welle 7, Auftrag N5. Der Cache selbst ist in
//! `narsil-cache` (`Cache`, `Anfrage`, `Antwort`, `Fund`) und dort
//! vollstaendig getestet. Hier steht nur die **Verdrahtung**: ein
//! prozess-globaler Ort, die Abschaltung ueber die Umgebung und die
//! Umrechnung zwischen `narsil_net::Response` und `narsil_cache::Antwort`.
//!
//! DIE REIHENFOLGE, und sie ist die ganze Aufgabe (Welle 7, N5):
//!
//! ```text
//!   Nachschlagen ──frisch──> Antwort aus dem Cache, **ohne Netz**
//!         │
//!         └──veraltet──> If-None-Match / If-Modified-Since mitschicken
//!                          │
//!                          ├──304──> gespeicherten Koerper nehmen
//!                          │          (RFC 9111 4.3.4) und ablegen
//!                          └──200──> ablegen, Antwort durchreichen
//! ```
//!
//! WARUM DER KOERPER **VOR** dem Entpacken abgelegt wird: der `Antwort`
//! in `narsil-cache` traegt, so wie es dort dokumentiert ist, „den Koerper
//! so, wie er auf der Leitung war — `Content-Encoding` steht noch drin".
//! Ein Cache, der den Koerper entpackt, gibt dem naechsten Besucher
//! eine Antwort, die er nicht entpacken kann.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use narsil_cache::{Anfrage, Antwort, Cache, Fund, Methode};

/// M3: die Umgebungsvariable, mit der der Cache abgeschaltet wird.
///
/// DER WERT `1` und nicht „irgendein Inhalt": die Vorgabe des Auftrags
/// N5 lautet `NARSIL_KEIN_CACHE=1`. Wer `NARSIL_KEIN_CACHE=0` setzt, haette
/// vermutlich das Gegenteil gemeint — die Variable heisst „kein Cache“,
/// also ist **jeder** andere Wert ebenfalls „kein Cache“. Ein Wert
/// gleich `1` waere eine Falle, die man erst im Fehlerfall findet.
pub const UMWELT_ABSCHALTEN: &str = "NARSIL_KEIN_CACHE";

/// M3: die Groesse des Platten-Caches, wenn nichts anderes gesagt wird.
///
/// DER WERT 256 MiB. GEMESSENE GRUNDLAGE: die groesste Seite aus dem
/// Live-Test am 30.09.2026 (14:24 UTC) war cloudflare mit 1326906 Byte
/// — 1,3 MiB. Selbst fuenfzig solcher Seiten sind 65 MiB, und 256 MiB
/// lassen Platz fuer eine Sitzung, ohne dass die Platte des Benutzers
/// belastet wird. `narsil-cache` hat intern `MAX_SPEICHER_BYTES` fuer den
/// Speicher; auf der Platte gilt die Grenze, die `Cache::mit_pfad`
/// bekommt, und die ist hier ausdruecklich gesetzt.
pub const PLATTE_GRENZE: u64 = 256 * 1024 * 1024;

/// DER WERT DER VORGABEN, und sie ist nicht leer:
///
/// * `cache: None` heisst „noch nicht gebaut“. Der erste Abruf baut ihn
///   ueber `cache_bauen`; `Some` heisst „da, benutze ihn“.
/// * `pfad: None` heisst „der Standardpfad“ (siehe `pfad_aus`).
/// * `abgeschaltet: None` heisst „nach `UMWELT_ABSCHALTEN` fragen“;
///   `Some(true)` heisst „ohne Cache“.
///   DER GRUND, DASS ES DEN ZUSTAND GIBT: die Umgebung wird nur beim
///   ersten Zugriff gelesen, und ein Test muss den Schalter setzen
///   koennen, ohne die Umgebung des Prozesses zu veraendern.
#[derive(Default)]
struct Zustand {
    /// Der Cache. `None`, wenn die Plattenstelle nicht zu oeffnen war —
    /// dann laeuft der Browser ohne Cache weiter statt zu sterben.
    cache: Option<Cache>,
    /// Ein abweichender Pfad, nur fuer Tests.
    pfad: Option<PathBuf>,
    /// Abschaltung. `None` heisst: nach `UMWELT_ABSCHALTEN` fragen.
    /// Ein Test setzt hier `Some(true)`, weil die Umgebung prozess-
    /// global ist und ein `set_var` nebenbei laufende Tests stoeren
    /// wuerde.
    abgeschaltet: Option<bool>,
}

static ZUSTAND: OnceLock<Mutex<Zustand>> = OnceLock::new();

fn zustand() -> MutexGuard<'static, Zustand> {
    ZUSTAND
        .get_or_init(|| Mutex::new(Zustand::default()))
        // `into_inner` statt `unwrap`: vergifteter Mutex heisst, dass ein
        // anderer Thread mitten im Cache gepanikt hat. Die Daten sind
        // trotzdem brauchbar — nur die Zaehlung der Platte kann
        // danebenliegen, und ein Browser, der danach nicht mehr ins
        // Netz kommt, ist schlimmer.
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// DER WEG ZUR ABSCHALTUNG, und warum sie an **zwei** Stellen steht.
///
/// 1. `UMWELT_ABSCHALTEN` (der Auftrag N5 nennt sie), und
/// 2. `abschalten_setzen` (Tests).
///
/// Der Testweg ist kein Komfort: `std::env::set_var` ist in Rust 2024
/// unsicher und wirkt auf **alle** Tests des Prozesses. Ein Test, der
/// die Umgebung setzt, wuerde damit die Reihenfolge der anderen Tests
/// bestimmen — genau die Sorte Kopplung, die ein Test nicht haben
/// darf.
pub fn ist_abgeschaltet() -> bool {
    if let Some(a) = zustand().abgeschaltet {
        return a;
    }
    std::env::var(UMWELT_ABSCHALTEN)
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Setzt die Abschaltung fest. `None` heisst: wieder nach der
/// Umgebung fragen.
///
/// added 30.09.2026, Welle 7 / N5. **Nur fuer Tests** — im Betrieb gibt
/// es `NARSIL_KEIN_CACHE`.
pub fn abschalten_setzen(wert: Option<bool>) {
    zustand().abgeschaltet = wert;
}

/// added 30.09.2026, Welle 7 / N5: die Umgebungsvariable, die den
/// Cache-Pfad ueberschreibt.
///
/// DER WEG ZU IHR: `Cache::mit_pfad` nimmt einen Pfad, und im Betrieb
/// ist das `narsil_cache::standard_pfad()`. Fuer **Tests** ist genau das
/// falsch: eine Testsuite, die in das echte `~/.cache/narsil` schreibt,
/// hinterlaesst Eintraege fuer `127.0.0.1`, und ein zweiter Lauf
/// derselben Suite kann dann eine **Antwort aus dem ersten Lauf**
/// bekommen — der Test prueft nicht mehr den Server, sondern sich
/// selbst. `tests/common/mod.rs` setzt diese Variable deshalb, bevor
/// es den ersten Testserver startet.
pub const UMWELT_PFAD: &str = "NARSIL_CACHE_PFAD";

/// Der Pfad aus der Umgebung, oder `None`.
fn pfad_aus_umgebung() -> Option<PathBuf> {
    std::env::var_os(UMWELT_PFAD)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// Setzt ein Verzeichnis als Cache-Pfad. `None` heisst: der
/// Standardpfad.
///
/// added 30.09.2026, Welle 7 / N5. **Nur fuer Tests** — ein Test, der
/// in den echten `~/.cache/narsil` schreibt, wuerde die Cache-Pruefung
/// des Benutzers zerstoeren.
pub fn pfad_setzen(pfad: Option<PathBuf>) {
    let mut z = zustand();
    // M3: WENN DER PFAD GLEICH BLEIBT, bleibt der Cache stehen.
    // `tests/common/mod.rs` ruft das bei **jedem** `TestServer::start`
    // auf; ohne diese Zeile wuerde der Cache dabei jedes Mal weggeworfen,
    // und kein Test koennte zwei Anlagen derselben Ressource machen.
    if z.pfad == pfad {
        return;
    }
    z.pfad = pfad;
    // Der alte Cache wird **nicht** weiterverwendet. Ein Wegwerfzeiger
    // auf eine `OnceLock`-Variable waere hier ein `Box::leak`;
    // stattdessen wird er ersetzt, und die alte Platte bleibt als Datei
    // liegen (das ist Sache des Tests, der sie angelegt hat).
    z.cache = None;
}

/// Der Pfad, **ohne** den Mutex zu nehmen.
///
/// DER GRUND FUER DIE ZWEI FORMEN, und es ist ein echter Fehler, nicht
/// eine Formalie: `Mutex` ist **nicht** wieder eintrittsfaehig. Ein
/// `pfad()`, das selbst sperrt, haengt sich selbst, sobald es aus einer
/// Funktion heraus aufgerufen wird, die den Zustand bereits haelt —
/// und genau das tat `cache_bauen`. Der erste Aufruf von `fetch_mit`
/// haengte dann **ewig** (gemessen am 30.09.2026, 19:5x: der Test
/// `frischer_eintrag_ohne_netz` lief ueber 200 Sekunden ohne Ende).
fn pfad_aus(z: &Zustand) -> PathBuf {
    if let Some(p) = z.pfad.clone() {
        return p;
    }
    // DIE REIHENFOLGE, und sie ist nicht beliebig: die Umgebung schlaegt
    // den Standardpfad, weil ein Test sie bewusst setzen kann. Der
    // Standardpfad (`$XDG_CACHE_HOME/narsil`, sonst `$HOME/.cache/narsil`)
    // ist der Betriebsfall und steht deshalb hinten.
    pfad_aus_umgebung().unwrap_or_else(narsil_cache::standard_pfad)
}

/// Der Pfad, den der Cache benutzt.
///
/// added 30.09.2026, Welle 7 / N5: `tests/common/mod.rs` fragt hier
/// nach, um zu sehen, ob schon ein Testverzeichnis gesetzt ist.
pub fn pfad() -> PathBuf {
    let z = zustand();
    pfad_aus(&z)
}

/// Baut den Cache, falls er noch nicht gebaut wurde.
///
/// DER WEG: `Cache::mit_pfad` mit der Grenze `PLATTE_GRENZE`. Scheitert
/// es (kein `$HOME`, keine Rechte), bleibt `cache == None`, und die
/// Zugriffe liefern `None` — der Browser arbeitet dann ohne Cache
/// weiter. Das ist Absicht: ein Cache ist eine Beschleunigung, kein
/// Bestandteil der Korrektheit.
fn cache_bauen(z: &mut Zustand) {
    if z.cache.is_some() {
        return;
    }
    // M3: `pfad_aus` statt `pfad` — siehe die Begruendung dort. Genau
    // dieser Aufruf war die Selbstsperre, die den ersten Abruf
    // aufgehaengt hat.
    let p = pfad_aus(z);
    z.cache = Cache::mit_pfad(&p, PLATTE_GRENZE).ok();
}

/// Nachschlagen. `None`, wenn es keinen Cache gibt.
///
/// M3: der `Mutex` wird nur fuer die Dauer des Zugriffs gehalten und
/// **nie** ueber einen Netzvorgang hinweg. Ein durchgehaltener
/// Cache-Lock wuerde zwei parallele Seitenabrufe serialisieren, und der
/// zweite wartete dann auf das Ergebnis des ersten statt auf das Netz.
pub fn nachschlagen(request: &crate::Request) -> Option<Fund> {
    if ist_abgeschaltet() {
        return None;
    }
    let mut z = zustand();
    cache_bauen(&mut z);
    Some(z.cache.as_mut()?.nachschlagen(&anfrage_aus(request)))
}

/// Den gespeicherten Inhalt nehmen, ohne auf Frische zu achten.
///
/// added 30.09.2026, Welle 7 / N5. WOFUER: nach einem `304`, wo
/// RFC 9111 4.3.4 den gespeicherten Inhalt verlangt. Siehe
/// `Cache::gespeichert`.
pub fn gespeichert(request: &crate::Request) -> Option<Antwort> {
    if ist_abgeschaltet() {
        return None;
    }
    let mut z = zustand();
    cache_bauen(&mut z);
    // M3: DAS ZWEITE `?` BEENDET, wenn der Server seinen gespeicherten
    // Inhalt nicht (mehr) hat; das erste, wenn es gar keinen Cache
    // gibt. Beides ist `None` fuer den Aufrufer — kein Cache und kein
    // Inhalt sind fuer den Browser dasselbe, er geht ins Netz.
    //
    // DER GRUND FUER DIE KURZE FORM: `Option<Antwort>` ist das, was
    // `gespeichert` zurueckgibt, und `?` beendet mit genau diesem
    // Wert. `Some(...?)` waere dasselbe mit zwei Zeichen mehr, und
    // clippy sagt es (gemessen am 30.09.2026, `needless_question_mark`).
    z.cache.as_mut()?.gespeichert(&anfrage_aus(request))
}

/// Eine Antwort ablegen.
///
/// DER AUFRUFPUNKT, und er ist der heikle: **`vor** `entpacke``. Wer
/// danach ablegt, speichert den entpackten Koerper mit einem
/// `Content-Encoding`-Header, und der naechste Besucher bekommt eine
/// Antwort, die er doppelt entpackt.
pub fn ablegen(request: &crate::Request, antwort: &crate::Response) {
    if ist_abgeschaltet() {
        return;
    }
    let mut z = zustand();
    cache_bauen(&mut z);
    if let Some(c) = z.cache.as_mut() {
        c.ablegen(&anfrage_aus(request), &antwort_aus(antwort));
    }
}

/// Alles loeschen (Speicher und Platte).
///
/// added 30.09.2026, Welle 7 / N5. **Nur fuer Tests** und fuer den
/// Aufrufer der den Cache steuert.
pub fn leeren() {
    let mut z = zustand();
    cache_bauen(&mut z);
    if let Some(c) = z.cache.as_mut() {
        c.leeren();
    }
}

/// Die Groessen des Caches: (Speicher-Byte, Speicher-Eintraege,
/// Platten-Byte, Platten-Eintraege). Ohne Cache viermal null.
pub fn groessen() -> (u64, usize, u64, usize) {
    let mut z = zustand();
    cache_bauen(&mut z);
    match z.cache.as_mut() {
        Some(c) => (
            c.bytes(),
            c.eintraege(),
            c.datei_bytes(),
            c.datei_eintraege(),
        ),
        None => (0, 0, 0, 0),
    }
}

/// Die Zaehler des Caches: **(Treffer, Misser)**.
///
/// added 02.10.2026 07:0x, R5i. DAS IST DER WUNSCH VON CACHE
/// (`team/WUNSCH_CACHE.txt`: „Cache-Treffer im selben Weg melden") und er
/// war eine **Zeile in dieser Datei**: `narsil_cache::Cache::statistik()`
/// existiert, und die Instanz liegt in `ZUSTAND` hier — nicht in
/// `narsil-cache`. Ein Wunsch, der eine fremde Datei braucht haette, waere
/// zwei Rollen an einem Strich.
///
/// DER WEG: `statistik()` weiterreichen, `None` als `(0, 0)`. Das ist
/// ehrlich statt `panic!`: ohne Cache (Plattenstelle nicht zu oeffnen)
/// gab es weder Treffer noch Misser, und `(0, 0)` sagt genau das.
///
/// WAS NICHT GESAGT WIRD: der Zaehler gehoert **einem Prozess** und
/// beginnt bei null. „Seit dem Start" ist die einzig zulaessige Lesart;
/// eine Aussage ueber „die letzte Anfrage" kann diese Zahl nicht treffen.
pub fn statistik() -> (u64, u64) {
    let mut z = zustand();
    cache_bauen(&mut z);
    match z.cache.as_ref() {
        Some(c) => c.statistik(),
        None => (0, 0),
    }
}

/// Macht aus einem `narsil_net::Request` die Form, die der Cache braucht.
///
/// DER WEG: nur `GET` ist eine Cache-Methode (RFC 9111 3). Fuer alle
/// anderen Methoden wird trotzdem eine `Anfrage` gebaut — sie wird nur
/// nirgends abgelegt, und `nachschlagen` liefert `Fehlt`.
pub fn anfrage_aus(request: &crate::Request) -> Anfrage {
    let methode = match request.method {
        crate::Method::Get => Methode::Get,
        crate::Method::Head => Methode::Head,
        // Release 10.10.2026: die neuen Methoden sind wie POST keine
        // Cache-Methoden (RFC 9111 §3: nur GET wird abgelegt).
        crate::Method::Post
        | crate::Method::Put
        | crate::Method::Delete
        | crate::Method::Patch
        | crate::Method::Options => Methode::Post,
    };
    Anfrage {
        methode,
        host: crate::host_ohne_schema(&request.host).to_string(),
        port: request.port,
        pfad: request.path.clone(),
        headers: request.headers.clone(),
    }
}

/// Macht aus einer `narsil_net::Response` die Form, die der Cache
/// speichert.
///
/// DER WICHTIGE PUNKT, und er ist in `narsil-cache` dokumentiert: der
/// Koerper wird **unveraendert** uebernommen. `fetch_mit` ruft diese
/// Funktion, **bevor** `entpacke` den Body auseinandernimmt.
pub fn antwort_aus(response: &crate::Response) -> Antwort {
    Antwort {
        status: response.status.code,
        headers: response.headers.clone(),
        koerper: response.body.clone(),
    }
}

/// Baut aus einer gespeicherten Antwort wieder eine `Response`.
///
/// DIE ZWEI FELDER, DIE MAN SETZEN MUSS, UND DIE JEDER VERGISSST:
/// `url` (der Aufrufer loest relative Links dagegen auf) und
/// `body_quelle` (der Aufrufer weiss daran, wie der Body transportiert
/// wurde). Ohne `url` waere eine Umleitung im Cache ein toter Link.
///
/// DER STATUS AUS DER KENNZahl: `Antwort.status` ist nur `u16`, also
/// wird der Grundtext aus einer kleinen Tabelle geholt. Ein
/// erfundener Grundtext waere eine Lüge, die man dem Benutzer anzeigt.
pub fn response_aus(antwort: &Antwort, url: &str) -> crate::Response {
    crate::Response {
        status: crate::StatusLine {
            version: "HTTP/1.1".into(),
            code: antwort.status,
            reason: grundtext(antwort.status).to_string(),
        },
        headers: antwort.headers.clone(),
        body: antwort.koerper.clone(),
        trailers: HashMap::new(),
        // added 30.09.2026, Welle 7 / N6: aus dem Cache kommen nur
        // die **zusammengefassten** Header, nicht die urspruengliche
        // `Set-Cookie`-Liste. Das ist richtig: ein gespeicherter Eintrag
        // wird nicht erneut gesetzt (RFC 9111 4.3 — der Cache liefert
        // den Inhalt, nicht die Nebenwirkungen). Wer sie braucht, liest
        // sie beim ersten Abruf aus dem Netz.
        set_cookie: Vec::new(),
        // M3: `ContentLength` stimmt, weil `ablegen` die Header
        // unveraendert speichert und der Koerper unveraendert
        // gespeichert wird. Ein Cache, der den Koerper entpackt haette,
        // wuerde hier luegen muessen — siehe den Dateikopf.
        body_quelle: crate::BodyQuelle::ContentLength,
        keep_alive: crate::KeepAlive::Bleibt,
        url: url.to_string(),
        entpackt: false,
    }
}

/// Der HTTP-Grundtext fuer die Statuscodes, die ein Cache speichert.
///
/// added 30.09.2026, Welle 7 / N5. DER GRUND, DASS HIER EINE TABELLE
/// UND KEIN TEXTBAU STEHT: `StatusLine.reason` geht bis in die
/// Statuszeile, die der Benutzer liest. Fuer die vier codes, die der
/// Cache nach RFC 9111 speichert, sind es vier Zeilen — fuer alles
/// andere waere eine Liste von hunderten der falsche Ort, und die Norm
/// schreibt vor, dass der Grundtext optional ist.
fn grundtext(code: u16) -> &'static str {
    match code {
        200 => "OK",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        206 => "Partial Content",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        308 => "Permanent Redirect",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        414 => "URI Too Long",
        501 => "Not Implemented",
        _ => "Unbekannt",
    }
}
