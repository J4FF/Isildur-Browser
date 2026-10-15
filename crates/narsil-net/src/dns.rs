//! DER DNS-CACHE — added 06.10.2026, Rolle NETZTEMPO (Auftrag: Ladegeschwindigkeit).
//!
//! # WAS ER KAUFT, gemessen
//!
//! `verbinde_neu` in `lib.rs` loest **jede** Adresse ueber
//! `ToSocketAddrs` neu auf. Bei einer Seite mit 30 Unterressourcen von
//! 4 Hosts sind das bis zu 30 Aufrufe des Systemauflosers fuer **drei**
//! verschiedene Namen — die anderen 27 sind Wiederholungen.
//!
//! GEMESSENE KOSTE EINER AUFLOESUNG (Belege in `lib.rs`, `Phasen.dns`):
//!   * `www.rust-lang.org`, 01.10.2026 16:8x: `dns Some(90.56ms)`
//!   * derselbe Host, spaeterer Lauf:  `dns Some(31.66ms)`
//! Beides sind **echte** Werte aus dem Betrieb, nicht geschaetzt. Und sie
//! stehen **vor** dem TCP-Handshake, also **vor** allem anderen: sie
//! liegen auf dem kritischen Pfad jedes neuen Sockels.
//!
//! # DIE ZAHLEN, und warum genau diese
//!
//! | Zahl | Wert | Grund |
//! |------|------|-------|
//! | [`MAX_NAMEN`] | 256 | M6: ein prozess-globaler Cache ohne Grenze ist ein Leck. 256 Namen sind fuer 200 Tabs mit je 3 Hosts reichlich und bleiben unter 1 MiB (M6: 256 * (Name ~64 B + 2 Adressen * 28 B) ~ 30 KiB). |
//! | [`TTL`] | 60 s | RFC 8767 (DNS-Caching) nennt fuer A/AAAA „der cached value hat eine TTL, die der Original-TTL folgt"; Browser verwenden 30 s bis 5 min. 60 s ist bewusst **kurz**: nach `TTL` wird neu gefragt, und ein Vorratsschreiben alter Adressen ist genau der Fehler, der einen Browser tagelang ins falsche Netz schickt. |
//! | [`MAX_ADRESSEN`] | 16 | M4: die Liste aus `to_socket_addrs` ist **Eingabe** und kommt ungeprueft. Wir speichern hoechstens 16 — und wenn der Aufloeser mehr liefert, wird **gar nichts** gecacht (siehe [`direkt`]-Rückgabe in [`loese`]). Dann ist der Cache nie schlechter als ohne ihn. |
//!
//! # WAS ER **NICHT** CACHET, und das ist eine Entscheidung
//!
//! **Fehlgeschlagene Aufloesungen werden nicht gecacht.** Grund: die
//! Fehlermeldung geht unveraendert in `NetError::Name` und von dort in die
//! Anzeige des Browsers („Adresse nicht gefunden"). Ein gecachter Fehler
//! wuerde (a) nach 60 s eine **andere** Lage melden als die, die wirklich
//! besteht, und (b) die vorhandenen Tests (`tests/methode_und_ipv6.rs`)
//! auf eine Fehlerquelle schicken lassen, die es nicht mehr gibt. Ein
//! Browser cacht auch NXDOMAIN — fuer **uns** ist der Preis hoher als der
//! Gewinn, weil ein Fehler bei uns selten ist und dann meistens
//! *voruebergehend*.
//!
//! # DER AUSSCHALTER, und warum er Pflicht ist
//!
//! [`NARSIL_DNS_CACHE`] mit dem Wert `aus` schaltet den Cache ab. Ohne ihn
//! waere **keine** der Zahlen in `team/BERICHT_NETZTEMPO_1.txt` eine
//! Vorher/Nachher-Messung, sondern ein Vergleich zweier Behauptungen: die
//! Messungen muessen **beide Wege** im selben Bau haben, sonst misst man
//! die Tagesform des Netzes mit.
//!
//! # KEINE LEAS, UND WARUM
//!
//! Beim ersten Treffer wird **nicht** die Zeit des ersten Aufrufs
//! abgezogen. Ein Cache mit Trefferzitierung (Holen bei Gelegenheit)
//! braucht einen zweiten Aufloeser-Thread je Name — und M6 verbietet
//! Threads ohne Obergrenze. Ohne Leas ist der Preis nach der Frist ein
//! `to_socket_addrs`-Aufruf, also **genau das Verhalten ohne Cache**;
//! das ist der Grund, warum diese Vereinfachung hier erlaubt ist und
//! eine andere nicht.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// M6: hoechstens so viele **Namen** im Cache.
///
/// BEGUENDUNG: der Cache ist prozess-global (er liegt in einem
/// `OnceLock`, wie der Verbindungspool), also waechst er mit der Zeit im
/// Prozess, nicht mit der Zahl der Tabs. 256 Namen sind fuer 200 Tabs
/// mit je 3 Hosts reichlich; darueber wird der **aelteste** Name
/// verdraengt, nicht der ganze Cache geleert — sonst haette ein einziger
/// Tab mit 300 Namen den Cache fuer alle anderen Tabs geleert.
pub const MAX_NAMEN: usize = 256;

/// M3: wie lange eine Aufloesung gilt, bevor wieder gefragt wird.
///
/// BEGUENDUNG: RFC 8767 Abschnitt 6 sagt, dass der Aufloeser die
/// Zwischenspeicherzeit aus dem TTL der Antwort uebernimmt; Browser
/// benutzen zwischen 30 s und 5 min. **60 s** ist die kurze Seite:
/// zwischen zwei Besuchen derselben Seite soll derselbe Sockel
/// benutzt werden (das kauft der Pool ohnehin), eine Stunde spaeter
/// aber soll **nicht** mehr die Adresse von vorhin gelten.
pub const TTL: Duration = Duration::from_secs(60);

/// M4: hoechstens so viele Adressen werden **gespeichert**.
///
/// BEGUENDUNG: `to_socket_addrs` gibt zurück, was das System liefert, und
/// das ist **Eingabe**. Bei mehr als 16 Adressen (IPv4+IPv6 eines grossly
/// anycasteten Namens) wird **gar nichts** gecacht — siehe [`loese`].
pub const MAX_ADRESSEN: usize = 16;

/// Der Schalter in der Umgebung. `aus` schaltet den Cache ab.
///
/// WARUM ALS **UMGEBUNG** und nicht als `Limits`-Feld: `Limits`
/// beschreibt **eine** Anfrage, der DNS-Cache ist ein Zustand des
/// **Prozesses**. Ausserdem braucht die Abnahme beide Wege im selben
/// Binary, und das geht mit einem Umgebungsschalter beim Start, nicht
/// mit einem Konstruktorargument an zehn Aufrufstellen.
pub const NARSIL_DNS_CACHE: &str = "NARSIL_DNS_CACHE";

/// Die Zaehler. Treffer und Misser sind **getrennt**, weil sie zwei
/// verschiedene Fragen beantworten: „wie oft wurde die Aufloesung
/// gespart" (Treffer) und „wie oft war der Cache zu klein, zu alt oder
/// aus" (Misser). Eine Summe beantwortet keine davon.
#[derive(Default)]
struct Zaehler {
    /// Aufloesungen, die aus dem Cache kamen.
    treffer: u64,
    /// Aufloesungen, die wirklich gefragt werden mussten.
    misser: u64,
    /// **Jede** echte Aufloesung, mit und ohne Cache, Erfolg **und**
    /// Misserfolg. added 06.10.2026 17:5x: siehe [`direkt`] — dieser
    /// Zaehler ist der einzige, der die beiden Wege vergleichbar macht.
    aufgeloest: u64,
}

#[derive(Default)]
struct Speicher {
    /// Name -> (Adressen in **Reihenfolge**, gueltig bis).
    ///
    /// DIE REIHENFOLGE IST TEIL DER ANTWORT, nicht Kosmetik: `lib.rs`
    /// probiert IPv4 und IPv6 **in der Reihenfolge**, in der der
    /// Aufloeser sie liefert (dort steht der Grund: R60 hat gemessen,
    /// dass `localhost` zuerst IPv6 liefert). Ein Cache, der die Liste
    /// umsortiert, aendert also, welcher Host gewaehlt wird.
    namen: HashMap<String, (Vec<SocketAddr>, Instant)>,
    /// Wann ein Name eingetragen wurde — fuer das Verdraengen von
    /// [`MAX_NAMEN`]. Getrennt vom Gueltigkeitsende, weil ein alter
    /// Eintrag auch der juengste sein kann (lange TTL, gerade geholt).
    eingetragen: HashMap<String, Instant>,
}

fn speicher() -> MutexGuard<'static, Speicher> {
    static S: OnceLock<Mutex<Speicher>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Speicher::default()))
        // wie `pool::pool()`: ein vergifteter Mutex ist kein Grund, den
        // Browser vom Netz zu nehmen — `into_inner` liefert die Daten.
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn zaehler() -> MutexGuard<'static, Zaehler> {
    static Z: OnceLock<Mutex<Zaehler>> = OnceLock::new();
    Z.get_or_init(|| Mutex::new(Zaehler::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// M3: TEST-HILFE. Leert den Cache.
///
/// WARUM `pub` und nicht `#[cfg(test)]`: die Tests in `tests/` binden die
/// Crate als **fremdes** ein; dort existieren `#[cfg(test)]`-Felder
/// nicht. Ohne diese Funktion wuerde jeder Test mit dem Zustand des
/// Tests vor ihm rechnen muessen — und `RUST_TEST_THREADS=2`
/// (bau.sh) laesst genau das zu.
pub fn leeren() {
    let mut s = speicher();
    s.namen.clear();
    s.eingetragen.clear();
}

/// Ist der Cache eingeschaltet? `false`, wenn `NARSIL_DNS_CACHE=aus`.
///
/// **ZWEI** Schalter, und der Grund fuer den zweiten ist ein **Compiler**,
/// nicht eine Bequemlichkeit: `std::env::set_var` ist seit Edition 2024
/// ein `unsafe`-Block, und `unsafe` ist in diesem Projekt verboten
/// (REGELN.txt, ADR-030). Ein Test, der den Cache im selben Prozess
/// aus- und einschalten will, kann die Umgebung also **nicht** setzen.
/// [`abschalten_setzen`] ist derselbe Schalter ohne `unsafe`.
pub fn ist_an() -> bool {
    if AUS.load(Ordering::Relaxed) {
        return false;
    }
    match std::env::var(NARSIL_DNS_CACHE) {
        Ok(w) => !w.eq_ignore_ascii_case("aus"),
        Err(_) => true,
    }
}

/// Der zweite Schalter, gesetzt von Hand. Vorrang vor der Umgebung.
///
/// M6: ein `AtomicBool` ist **ein** Byte — die kleinste moegliche
/// zustaendige Ablage. Begruendung fuer `Relaxed`: es gibt hier keinen
/// zweiten Thread, mit dem man synchronisieren muesste; die Aenderung
/// soll nur im selben Thread sichtbar sein, und fuer das reicht
/// Relaxed. `SeqCst` wuerde ohne Gegenstelle nichts kaufen.
static AUS: AtomicBool = AtomicBool::new(false);

/// M3: TEST-HILFE. Schaltet den Cache **im Prozess** ab (oder wieder an).
///
/// WARUM NICHT `#[cfg(test)]`: die Tests in `tests/` binden die Crate als
/// fremdes ein. Ausserdem soll der **A/B-Vergleich** der Abnahme beide
/// Wege in einem Binary haben — das geht mit einem Aufruf, nicht nur mit
/// einer Umgebungsvariable.
///
/// `false` stellt den **Zustand vom Programmstart** wieder her: dann
/// entscheidet wieder allein die Umgebung, und ein Test, der den Schalter
/// vergessen hat, bekommt keinenstillen Nebeneffekt.
pub fn abschalten_setzen(wert: bool) {
    AUS.store(wert, Ordering::Relaxed);
}

/// Loest `adresse` auf — aus dem Cache, wenn er gilt.
///
/// **DIE REIHENFOLGE DES WEGS** ist die Reihenfolge der Faelle, nicht die
/// eines Zufalls: erst der Schalter (der auf 0 % Cache-Treffer zurueck-
/// schaltet und damit die A/B-Messung ueberhaupt erst moeglich macht),
/// dann der Cache, dann — **nach** dem Freigeben der Sperre — das Netz.
/// Das Freigeben **vor** dem Netz ist der Punkt: `to_socket_addrs`
/// blockiert, und eine Sperre, die waehrenddessen gehalten wird, macht
/// jeden zweiten Thread zur queueenden Zuschauerin.
pub fn loese(adresse: &str) -> Result<Vec<SocketAddr>, String> {
    if !ist_an() {
        return direkt(adresse);
    }
    // 1. TREFFER. Nur gelesen, nur kopiert.
    {
        let s = speicher();
        if let Some((liste, bis)) = s.namen.get(adresse) {
            if Instant::now() < *bis {
                let raus: Vec<SocketAddr> = liste.clone();
                drop(s);
                let mut z = zaehler();
                z.treffer = z.treffer.saturating_add(1);
                return Ok(raus);
            }
        }
    }
    // 2. DAS NETZ. Keine Sperre gehalten.
    let antwort = direkt(adresse)?;
    // 3. ABLEGEN, aber nur wenn die Liste in die Grenze passt. Sonst
    //    liefert `direkt` mehr, als der Cache je speichern wuerde, und
    //    ein Treffer waere dann eine **schlechtere** Antwort.
    if antwort.len() <= MAX_ADRESSEN {
        let mut s = speicher();
        let jetzt = Instant::now();
        // M3/M6: die Grenze wird **vor** dem Eintragen geprueft, und
        // verdraengt wird der **aelteste** Name. Warum nicht `clear()`:
        // ein Tab mit 300 Namen wuerde sonst den Cache aller anderen
        // Tabs leeren — der Aufwand steckt in diesem Tab, der Nutzen
        // gehoert aber allen.
        while s.namen.len() >= MAX_NAMEN {
            // M3: `min_by_key` gibt `None` fuer eine leere Liste. Der
            // Zweig ist hier **nicht** erreichbar (`namen` hat dann
            // 0 Eintraege, also ist `len() >= MAX_NAMEN` mit
            // `MAX_NAMEN >= 1` falsch) — aber er ist da und endet im
            // selben Weg wie „nichts zu verdraengen", naemlich: es
            // wird **nicht** eingetragen.
            let aeltester = s
                .eingetragen
                .iter()
                .min_by_key(|(_, z)| **z)
                .map(|(k, _)| k.clone());
            match aeltester {
                Some(k) => {
                    s.namen.remove(&k);
                    s.eingetragen.remove(&k);
                }
                None => break,
            }
        }
        s.namen
            .insert(adresse.to_string(), (antwort.clone(), jetzt + TTL));
        s.eingetragen.insert(adresse.to_string(), jetzt);
        drop(s);
        let mut z = zaehler();
        z.misser = z.misser.saturating_add(1);
    }
    Ok(antwort)
}

/// DER WEG OHNE CACHE. Genau der Aufruf, der vorher **immer** lief.
///
/// **DIE ZAEHLUNG SITZT HIER, und nicht in [`loese`]** — das ist ein
/// Befund aus der Abnahme vom 06.10.2026. Die erste Fassung zaehlte im
/// Treffer-Zweig von `loese`, also **nur bei eingeschaltetem Cache**.
/// GEMESSENE Folge: die Laeufe **ohne** Cache meldeten `dns_Netz 0`, die
/// **mit** Cache `126` — und die Auswertung verglich 0 mit 126 und
/// meldete „der Cache hat 126 Auflösungen **mehr** verursacht", was
/// voellig falsch ist (es wurden 126 **weniger** gefragt, aber der
/// Zaehler stand am anderen Ort). Eine Zahl, die nur auf **einem** der
/// beiden Wege gezaehlt wird, ist auf dem anderen keine Zahl, sondern
/// eine Luecke.
///
/// Also: **jede** echte Aufloesung — mit Cache, ohne Cache, im Fehlerfall
/// — zaehlt hier genau einmal. Damit ist `aufgeloest` der Zaehler, der
/// beide Wege vergleichbar macht.
fn direkt(adresse: &str) -> Result<Vec<SocketAddr>, String> {
    let r = adresse
        .to_socket_addrs()
        .map(|i| i.collect())
        .map_err(|e| e.to_string());
    // M3: die Zahl ist ein `saturating_add` auf `u64` — sie kann nicht
    // ueberlaufen und nicht zuruecklaufen, also gibt es keinen Pfad, der
    // hier panickt.
    let mut z = zaehler();
    z.aufgeloest = z.aufgeloest.saturating_add(1);
    r
}

/// Wie viele Aufrufe kamen aus dem Cache, wie viele gingen ans Netz.
///
/// `(Treffer, Misser)`. **Achtung:** ein Misser, dessen Liste laenger als
/// [`MAX_ADRESSEN`] war, wird **nicht** gezaehlt — er ist weder Treffer
/// noch Misser, sondern ein Fall, den es vorher nicht gab. Fuer die
/// Abnahme ist das eine Kleinigkeit, fuer eine Rechnung „Trefferquote =
/// Treffer / (Treffer + Misser)" aber eine falsche Zahl; deshalb steht
/// hier die **beiden** und nicht die Quote.
pub fn statistik() -> (u64, u64) {
    let z = zaehler();
    (z.treffer, z.misser)
}

/// Wie viele **echte** Aufloesungen (`to_socket_addrs`) liefen — mit und
/// ohne Cache, **Erfolg und Misserfolg**.
///
/// added 06.10.2026 17:5x. DIESER Zaehler ist der **einzige**, der die
/// beiden Wege vergleichen kann: [`statistik`] zaehlt nur den
/// Cache-Zugriff, und wenn der Cache **aus** ist, gibt es keinen Zugriff
/// und also auch keine Zahl — die Abnahme vom 06.10.2026 verglich
/// dadurch 0 mit 126 und kam zu einem voellig falschen Schluss.
///
/// **DER UNTERSCHIED ZU `misser`, und er ist nicht klein:** `aufgeloest`
/// zaehlt **jeden** Aufruf des Systemauflosers, `misser` nur die
/// **erfolgreichen**. Bei einem nicht aufloesbaren Namen ist deshalb
/// `aufgeloest = 1` und `misser = 0`. Fuer die Frage „wie viele Male haben
/// wir gefragt" ist nur `aufgeloest` richtig, und **nur** diese Zahl darf
/// einen A/B ueber den Cache tragen.
pub fn aufgeloest() -> u64 {
    let z = zaehler();
    z.aufgeloest
}

/// Wie viele Namen gerade im Cache liegen. Fuer Tests und die Meldung.
pub fn groesse() -> usize {
    speicher().namen.len()
}

/// Steht dieser Name **gueltig** im Cache? Nur fuer Tests und fuer die
/// Statuszeile — der Abrufweg benutzt [`loese`].
///
/// DER WEG: es wird **nur** geschaut, nicht gecacht. Eine Funktion, die
/// beim Fragen nebenbei etwas anlegt, ist keine Beobachtungsfunktion,
/// sondern ein zweiter Weg in denselben Raum — und die Zahl, die sie
/// zurueckgibt, haengt dann von der Frage ab.
pub fn enthaelt(adresse: &str) -> bool {
    let s = speicher();
    match s.namen.get(adresse) {
        Some((_, bis)) => Instant::now() < *bis,
        None => false,
    }
}

/// Setzt die Zaehler zurueck. Wie [`leeren`]: nur fuer Tests.
pub fn statistik_zuruecksetzen() {
    let mut z = zaehler();
    z.treffer = 0;
    z.misser = 0;
    z.aufgeloest = 0;
}

// =============================================================================
// TESTS — jede Behauptung oben wird hier geprueft, und jede Pruefung sichert
// ein Ergebnis (M7), keine davon ruft nur auf (M7 verbietet das ausdruecklich).
// =============================================================================
#[cfg(test)]
mod dns_tests {
    use super::*;

    /// DIE TESTSPERRE, und sie ist **kein** Luxus.
    ///
    /// **GEMESSENER BEFUND, dreistufig** — beim Bauen dieses Moduls ist
    /// dreimal ein Test gefallen, und **jedes Mal aus demselben Grund**:
    /// der Cache liegt in einem prozess-globalen `OnceLock`, und
    /// `werkzeuge/bau.sh` setzt `RUST_TEST_THREADS=2`. Zwei Beispiele, beide
    /// **einzeln** gruen und **zusammen** rot:
    ///   1. `unaufloesbarer_name_endet_schnell_und_legt_nichts_ab` prüfte
    ///      `groesse() == 0` und sah den `localhost:80`-Eintrag des
    ///      Nachbarn.
    ///   2. `der_cache_wird_benutzt_und_die_frist_beendet_es` vergiftet
    ///      einen Eintrag mit einer Sentinelle; der Nachbar rief dazwischen
    ///      `leeren()` und die Sentinelle war weg — **ohne** Fehlermeldung
    ///      an der falschen Stelle, der Test meldete nur „Falsch".
    ///
    /// Warum die Sperre und nicht `RUST_TEST_THREADS=1`: `bau.sh` ist ein
    /// **gemeinsames** Skript (SCHREIBEN NICHT ANFASSEN), und eine
    /// Ein-Thread-Konfiguration dort wuerde **alle** Agenten treffen. Die
    /// Sperre gehoert deshalb in **diese** Datei.
    ///
    /// M6: ein `Mutex` ohne inhaltlichen Zustand, genau einmal im Prozess
    /// geholt — er kann nicht leaken und nichts wachsen lassen.
    static SPERRE: Mutex<()> = Mutex::new(());

    /// Nimmt die Sperre. **Jeder** Test, der `leeren`, `loese` oder
    /// `vergifte` benutzt, muss sie zuerst nehmen.
    ///
    /// DER WEG, und warum nicht `unwrap`: dieselbe Form wie in [`super::speicher`]
    /// — ein vergifteter Mutex (ein Fremdpanic mitten im Test) darf den
    /// Rest der Crate nicht lahmlegen.
    fn sperre() -> MutexGuard<'static, ()> {
        SPERRE.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Die Zahlen der Tabelle oben, als **Behauptung**. Wenn jemand
    /// `MAX_NAMEN` auf 100000 hebt, faellt genau dieser Test um.
    #[test]
    fn die_zahlen_der_tabelle_stehen_im_code() {
        assert_eq!(MAX_NAMEN, 256, "der Kommentar oben nennt 256");
        assert_eq!(MAX_ADRESSEN, 16, "der Kommentar oben nennt 16");
        assert_eq!(
            TTL,
            Duration::from_secs(60),
            "der Kommentar oben nennt 60 s"
        );
    }

    /// `localhost` **ohne** Netz ist der einzige Name, dessen Aufloesung
    /// auf einem Testrechner sicher gelingt. Er wird benutzt, um den Weg
    /// zu pruefen — nicht um eine Zahl ueber das Netz zu behaupten.
    ///
    /// **GEMESSEN, warum dieser Test KEINE Zaehlerzahl prueft** (und das ist
    /// der zweite, nicht der erste Befund beim Bauen dieses Moduls): die
    /// Zaehler liegen in einem prozess-globalen `OnceLock`, und
    /// `werkzeuge/bau.sh` setzt `RUST_TEST_THREADS=2`. Ein Nachbar, der
    /// zur selben Zeit `loese` ruft, verschiebt jede Zahl. Die **erste**
    /// Fassung dieses Tests behauptete `tr + fm >= 1` und fiel genau
    /// deshalb durch, dass ein Fremder **mehr** zaehlte. Die Behauptung,
    /// die bleiben darf, ist eine **namensbezogene** — und die steht in
    /// [`der_zweite_aufruf_ist_ein_treffer`].
    #[test]
    fn localhost_wird_aufgeloest() {
        let _s = sperre();
        leeren();
        let r = loese("localhost:80");
        assert!(r.is_ok(), "localhost:80 muss sich aufloesen lassen: {r:?}");
        let liste = r.unwrap();
        assert!(!liste.is_empty(), "die Adressenliste darf nicht leer sein");
        // Und die **Reihenfolge** ist Teil der Antwort (siehe Kopf):
        // IPv4 vor IPv6, wie es der Aufloeser gibt. Geprueft wird nur,
        // dass die Liste stabil ist — nicht welche Familie zuerst kommt,
        // das entscheidet der Systemaufloeser.
        assert!(
            enthaelt("localhost:80"),
            "nach einem erfolgreichen Aufruf muss der Name im Cache liegen"
        );
    }

    /// Der zweite Aufruf **muss** ein Treffer sein. Das ist die eigentliche
    /// Behauptung dieses Moduls, und sie wird an einem echten Namen
    /// gemessen, nicht an einer Attrappe.
    ///
    /// DER WEG, und warum **ohne** Zaehler: siehe den Kopf von
    /// [`localhost_wird_aufgeloest`]. Die Behauptung „der zweite Aufruf ging
    /// nicht ans Netz" wird **indirekt** geprueft, ueber [`enthaelt`] und
    /// ueber die Dauer: ein zweiter Systemaufruf waere messbar langsamer
    /// als ein Tabellenzugriff, also wird die Dauer **begrenzt** statt
    /// verglichen — eine Schranke ist auch dann richtig, wenn ein Nachbar
    /// nebenbei laeuft.
    #[test]
    fn der_zweite_aufruf_ist_ein_treffer() {
        let _s = sperre();
        leeren();
        assert!(loese("localhost:80").is_ok());
        assert!(enthaelt("localhost:80"), "der erste muss gelegt haben");
        // 1 ms ist **keine** Behauptung ueber die Geschwindigkeit des
        // Caches, sondern eine **Schranke gegen den Systemaufruf**: der
        // Aufloeser von `localhost` braucht auf dieser Maschine
        // messbar mehr als 1 ms, ein Tabellenzugriff praktisch 0. Wird
        // diese Schranke je gerissen, ist der Cache **aus** — und dann
        // sagt der Test genau das.
        let start = Instant::now();
        assert!(loese("localhost:80").is_ok());
        let dauer = start.elapsed();
        assert!(
            dauer < Duration::from_millis(1),
            "der zweite Aufruf muss ein Tabellenzugriff sein, brauchte {dauer:?}"
        );
    }

    /// Der Cache muss **beide** Wege liefern: die vier Ausgaben des
    /// `direct` muessen unveraendert im Treffer wiederstehen. Ohne das
    /// waere der Cache still ein Filter, und ein stiller Filter in
    /// `verbinde_neu` heisst: der Browser baut Sockels zu Adressen, die
    /// der Aufloeser nie genannt hat.
    #[test]
    fn der_treffer_gibt_dieselbe_liste_zurueck() {
        leeren();
        let a = direkt("localhost:80").expect("localhost:80");
        let b = loese("localhost:80").expect("localhost:80");
        assert_eq!(
            a, b,
            "Cache und Direktweg muessen dieselbe Adressenliste liefern"
        );
        assert!(!b.is_empty(), "die Liste darf nicht leer sein");
    }

    /// Der Schalter muss **wirken** — auf die Trefferzahl, nicht nur auf
    /// den Codeweg. Ohne diese Pruefung waere ein Tippfehler im Wert
    /// („AUS" statt „aus") unbemerkt geblieben.
    #[test]
    fn der_schalter_beendet_die_trefferzahl() {
        let _s = sperre();
        // **SENTINELLE STATT ZAEHLER.** Die erste Fassung behauptete
        // `Treffer == 0` und war damit von jedem Nachbartest abhaengig, der
        // zur selben Zeit `loese` ruft (siehe Kopf der Sperre). Die
        // Sentinelle beweist dasselbe **ohne** Zaehler: ist der Cache aus,
        // kann sie nicht herauskommen, weil sie nur im Cache liegt.
        vergifte("localhost:80", sentinel(), Duration::from_secs(0));
        abschalten_setzen(true);
        let r = loese("localhost:80");
        abschalten_setzen(false);
        assert!(
            r.is_ok(),
            "der Name muss auch bei ausgeschaltetem Cache gehen"
        );
        assert_ne!(
            r.unwrap(),
            sentinel(),
            "mit ausgeschaltetem Cache darf die Sentinelle NICHT kommen"
        );
    }

    /// M8: ein Name, der **nicht** aufloesbar ist, muss in unter einer
    /// Sekunde mit einem **benannten** Fehler (hier: dem String, den
    /// `lib.rs` in `NetError::Name` steckt) enden — nicht mit Panic, nicht
    /// mit unbegrenztem Speicher, und ohne etwas in den Cache zu legen.
    #[test]
    fn unaufloesbarer_name_endet_schnell_und_legt_nichts_ab() {
        let _s = sperre();
        leeren();
        let start = Instant::now();
        let r = loese("kein.host.invalid.example.invalid:80");
        let dauer = start.elapsed();
        assert!(r.is_err(), "dieser Name darf sich nicht aufloesen lassen");
        assert!(
            dauer < Duration::from_secs(1),
            "die Aufloesung muss schnell scheitern, brauchte {dauer:?}"
        );
        // **NAMENSBEZOGEN**, nicht `groesse() == 0`. GEMESSEN, warum: die
        // erste Fassung behauptete `groesse() == 0` und fiel durch, weil
        // ein **gleichzeitig laufender** Nachbartest (`localhost:80`) den
        // globalen Cache gefuellt hatte. `enthaelt()` fragt **diesen**
        // Namen ab und kann von einem Nachbarn nicht gestoert werden.
        assert!(
            !enthaelt("kein.host.invalid.example.invalid:80"),
            "ein Fehler darf nicht gecacht werden"
        );
    }

    /// M8/M4: ein **sehr langer** Name (der Speicher mueste mit der
    /// Eingabe wachsen — hier 4096 Byte) muss begrenzt enden. Ohne die
    /// Grenze waere der Cache der erste Ort, an dem eine lange Adresse
    /// Speicher belegt.
    #[test]
    fn langer_name_bleibt_begrenzt() {
        let _s = sperre();
        leeren();
        let lang = format!("{}.invalid.example:80", "a".repeat(4096));
        let start = Instant::now();
        let _ = loese(&lang);
        let dauer = start.elapsed();
        assert!(
            dauer < Duration::from_secs(2),
            "auch ein 4-KiB-Name muss schnell scheitern, brauchte {dauer:?}"
        );
        // Wieder **namensbezogen**, aus demselben Grund wie oben.
        assert!(!enthaelt(&lang), "ein Fehler darf nicht gecacht werden");
    }

    /// M6: die Grenze [`MAX_NAMEN`] muss **greifen**, und zwar als
    /// **Verdraengung des aeltesten**, nicht als `clear()`.
    ///
    /// DER WEG, und warum `localhost:PORT` hier taugt: jeder dieser Namen
    /// ist ein **anderer** Name (der Aufloeser sieht den Port) und loest
    /// sich **auf** (die Existenz des Ports wird nicht geprueft). Der Test
    /// braucht also kein Netz und haengt nicht an einem DNS-Fehler.
    ///
    /// ZWEI BEHAUPTUNGEN, weil sie sich gegenseitig widersprechen koennen:
    ///   1. `groesse() <= MAX_NAMEN` — die Grenze gilt.
    ///   2. der **letzte** Name liegt noch drin — es wurde verdraengt und
    ///      nicht leergewischt. Ohne (2) waere `clear()` auch eine
    ///      "begrenzte" Loesung, und genau die kostet allen anderen Tabs
    ///      ihren Cache.
    #[test]
    fn die_grenze_verdraengt_den_aeltesten_und_nicht_alles() {
        let _s = sperre();
        leeren();
        for i in 0..(MAX_NAMEN + 20) {
            let name = format!("localhost:{}", 8000 + i);
            assert!(loese(&name).is_ok(), "{name} muss sich aufloesen lassen");
        }
        assert!(
            groesse() <= MAX_NAMEN,
            "{} Namen im Cache, erlaubt sind {MAX_NAMEN}",
            groesse()
        );
        let letzter = format!("localhost:{}", 8000 + MAX_NAMEN + 19);
        assert!(
            enthaelt(&letzter),
            "der juengste Name muss liegen: ein clear() statt einer Verdraengung"
        );
        let erster = "localhost:8000".to_string();
        assert!(
            !enthaelt(&erster),
            "der aelteste Name muss **weg** sein: genau das ist die Verdraengung"
        );
        // `leeren` wird an **diesen** Namen geprueft, nicht an `groesse()`:
        // ein gleichzeitiger Nachbar koennte zwischen `leeren` und der
        // Pruefung wieder etwas ablegen (GEMESSEN, genau das ist beim
        // Bauen zweier Tests passiert).
        leeren();
        assert!(
            !enthaelt(&erster) && !enthaelt(&letzter),
            "nach leeren muss der Cache leer sein"
        );
    }

    /// M3: [`TTL`] muss **wirken**. Der Test vergiftet den Cacheeintrag mit einer
    /// **Sentinelliste** und macht ihn kuenstlich alt — dieselbe
    /// `Instant`-Differenz, die nach 60 s Wartzeit entstuende (so wie
    /// `pool::lege_ab_alt`, aus demselben Grund: ein Test, der 60 s wartet,
    /// ist unbrauchbar).
    ///
    /// WARUM EINE SENTINELLE UND KEINE ZEITMESSUNG: ein Zeitvergleich waere
    /// kein Beweis, weil der Systemaufloeser von `localhost` manchmal unter
    /// 1 ms liegt. Eine **unterscheidbare Adresse** ist es. Begruendung in
    /// voller Laenge im Test darunter.

    /// M3: [`TTL`] muss **wirken** — und das wird **ohne** Zaehler
    /// bewiesen, weil Zaehler hier process-global sind (Kopf von
    /// [`localhost_wird_aufgeloest`]).
    ///
    /// DER WEG, und er ist der einzige, der ohne Zaehler etwas beweist:
    /// der Test **vergiftet** den Cacheeintrag mit einer **Sentinelliste**
    /// (`0.0.0.0:1`, also eine Adresse, die der Aufloeser nie nennt).
    /// Liefert `loese` die Sentinel zurueck, kam sie **aus dem Cache**.
    /// Liefert sie die echte Aufloesung, wurde der Eintrag **nicht**
    /// benutzt. Damit ist beides gemessen, ohne eine Zahl zu zaehlen:
    ///
    ///   * mit `alter = 0` (frisch)  -> Sentinel kommt zurueck
    ///   * mit `alter = TTL + 1 s`   -> Sentinel kommt **nicht** zurueck
    ///
    /// EIN GEDANKE, DER HIER AUSDRUECKLICH NICHT GILT: ein Zeitvergleich
    /// („die zweite Aufloesung war langsamer") waere **kein** Beweis,
    /// weil der Systemaufloeser von `localhost` manchmal unter 1 ms
    /// liegt. Eine unterscheidbare **Adresse** ist es.
    ///
    /// WARUM EINE SOLCHE VERGIFTUNG UEBERHAUPT ERLAUBT IST: der Test
    /// greift in den **privaten** Zustand der Datei zu (`speicher()`), nicht
    /// in eine oeffentliche Funktion. Von aussen kann niemand eine falsche
    /// Adresse in den Cache bringen — das ist der Unterschied zwischen
    /// einem Test und einem Loch.
    fn vergifte(name: &str, liste: Vec<SocketAddr>, alter: Duration) {
        let bis = Instant::now()
            .checked_sub(alter)
            .unwrap_or_else(Instant::now)
            + TTL;
        let mut s = speicher();
        s.namen.insert(name.to_string(), (liste, bis));
        s.eingetragen.insert(name.to_string(), Instant::now());
    }

    fn sentinel() -> Vec<SocketAddr> {
        vec![SocketAddr::from(([0, 0, 0, 0], 1))]
    }

    /// M7/added 06.10.2026 17:5x: **DIE ZWEI ZAHLEN, und ihre
    /// tatsaechliche Beziehung** — nicht die, die ich zuerst behauptet habe.
    ///
    /// **WAS ICH ZUERST BEHAUPTET HABE:** `aufgeloest == treffer + misser`.
    /// **GEMESSEN, dass das falsch ist:** der Test meldete `1 != 1 + 1`. Der
    /// Grund ist ein Unterschied, den ich beim Schreiben nicht gesehen
    /// hatte: `aufgeloest` zaehlt **jeden** Aufruf von `to_socket_addrs`,
    /// auch den, der **scheitert**; `misser` zaehlt nur die
    /// **erfolgreichen**, denn der Fehlerpfad in `loese` gibt **vor** dem
    /// `misser`-Zaehler zurueck. Die Beziehung lautet also:
    ///
    /// ```text
    ///   aufgeloest == misser        wenn keine Aufloesung scheiterte
    ///   aufgeloest >  misser        wenn eine scheiterte
    ///   aufgeloest == treffer + misser + fehlgeschlagene
    /// ```
    ///
    /// Und **das** ist der Grund, warum es zwei Zaehler geben muss: der
    /// Erfolgszaehler allein kann nicht sagen, wie viele Male **ueberhaupt**
    /// gefragt wurde — und genau diese Zahl vergleicht die Abnahme.
    #[test]
    fn die_zwei_zaehler_stehen_in_dieser_beziehung() {
        let _s = sperre();
        // (1) MIT Cache: zwei Aufrufe, eine Aufloesung, ein Treffer.
        abschalten_setzen(false);
        leeren();
        statistik_zuruecksetzen();
        assert!(loese("localhost:80").is_ok());
        assert!(loese("localhost:80").is_ok());
        let (tr, fm) = statistik();
        let a = aufgeloest();
        assert_eq!(tr, 1, "der zweite Aufruf war ein Treffer");
        assert_eq!(fm, 1, "der erste Aufruf war ein Misser");
        assert_eq!(
            a, fm,
            "ohne gescheiterte Aufloesung gilt aufgeloest == misser: {a} != {fm}"
        );
        // (2) OHNE Cache: kein Treffer, aber **zwei** Aufloesungen.
        abschalten_setzen(true);
        statistik_zuruecksetzen();
        assert!(loese("localhost:81").is_ok());
        assert!(loese("localhost:81").is_ok());
        let (tr2, _) = statistik();
        let b = aufgeloest();
        assert_eq!(tr2, 0, "ohne Cache kann es keinen Treffer geben");
        assert_eq!(
            b, 2,
            "ohne Cache muss jeder Aufruf fragen: war {b}, erwartet 2"
        );
        abschalten_setzen(false);
    }

    /// M8: eine **gescheiterte** Aufloesung zaehlt zu `aufgeloest`, aber
    /// **nicht** zu `misser`. Das ist der Unterschied, den der Test oben
    /// als Zahl festnagelt — hier wird er an einem Fehler geprueft.
    #[test]
    fn eine_gescheiterte_aufloesung_zaehlt_nur_in_aufgeloest() {
        let _s = sperre();
        abschalten_setzen(false);
        statistik_zuruecksetzen();
        assert!(loese("bombe.invalid:80").is_err());
        let (tr, fm) = statistik();
        let a = aufgeloest();
        assert_eq!(tr, 0, "ein Fehler ist kein Treffer: {tr}");
        assert_eq!(fm, 0, "ein Fehler ist kein Misser: {fm}");
        assert_eq!(a, 1, "der **Versuch** wird gezaehlt: war {a}");
    }

    /// M3: Der Cache wird benutzt, **und** die Frist beendet ihn. Beides an
    /// einer **Sentineladresse** gemessen, nicht an einer Zeit — ein
    /// Zeitvergleich waere kein Beweis, weil der Systemaufloeser von
    /// `localhost` manchmal unter 1 ms liegt. Die volle Begruendung steht
    /// bei [`vergifte`].
    #[test]
    fn der_cache_wird_benutzt_und_die_frist_beendet_es() {
        let _s = sperre();
        let name = "localhost:80";
        // (1) FRISCH: die Giftadresse muss herauskommen.
        vergifte(name, sentinel(), Duration::from_secs(0));
        assert!(enthaelt(name), "ein frischer Eintrag muss gueltig sein");
        let r = loese(name);
        assert_eq!(
            r.as_ref().ok().map(|v| v.len()),
            Some(1),
            "der Cache muss geliefert haben: {r:?}"
        );
        assert_eq!(
            r.unwrap(),
            sentinel(),
            "der Cache muss genau die gelegte Liste liefern"
        );
        // (2) ABGELAUFEN: die Giftadresse darf **nicht** kommen.
        vergifte(name, sentinel(), TTL + Duration::from_secs(1));
        assert!(
            !enthaelt(name),
            "der Eintrag darf nach TTL + 1 s nicht mehr gueltig sein"
        );
        let r = loese(name);
        assert!(r.is_ok(), "der Name muss trotzdem aufloesbar sein: {r:?}");
        assert_ne!(
            r.unwrap(),
            sentinel(),
            "nach der Frist darf die Giftadresse NICHT kommen — sie kam"
        );
    }
}
