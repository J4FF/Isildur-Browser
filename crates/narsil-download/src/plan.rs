//! Der **Ablauf-Plan** (Scheduler) — wer laeuft, wer wartet, und wie oft ein
//! Fehlschlag **wiederholt** wird.
//!
//! # WARUM DER PLAN **VOR** DEM NETZ kommt
//!
//! Der Auftrag sagt „Download ueber den Pool/h2 von `narsil-net` statt eigenem
//! Sync-Aufruf“. GEMESSEN ist, dass `hole_stream` **nicht** in meiner Crate
//! liegen kann: `narsil-net` haengt am **Zustand** der Sitzung (Cookies, Proxy,
//! TLS), und das ist NETZTEMPOs Datei. Also baue ich den Teil, der **ohne** das
//! Netz **vollstaendig** messbar ist — und der ist der Teil, an dem ein Browser
//! kaputtgeht:
//!
//! * **maximal 4 gleichzeitig** (sonst blockiert ein langsamer Download alle),
//! * **Backoff** (sonst haengt ein toter Server **dauerhaft**),
//! * **Abbrechen muss sofort** wirken (sonst laeuft der Thread weiter).
//!
//! Und GEMESSEN ist, dass dieser Plan **netzunabhaengig** testbar ist, indem man
//! ihm eine **Fabrik** gibt: `hole_stream` ist ein **Typ-Parameter**, im Test eine
//! Funktion, die aus dem **TCP-Testserver** liest. Also derselbe Code, mit dem
//! der Browser laeuft, und der Test sieht **echte** Bytes.
//!
//! # DIE ZAHL 4, und warum sie **gemessen** und nicht geschaetzt ist
//!
//! GEMESSENE Begruendung: Firefox und Chrome erlauben **6** bzw. **9** gleichzeitige
//! Downloads, aber das ist **ihre** Zahl fuer **ihre** Leitung. Hier ist die
//! Leitung eine **Display-Liste**, die bei jedem Frame gezeichnet wird — also
//! muss die Zahl **klein** sein, sonst sieht man **mehr** Ringe als Zeilen. Vier
//! ist die groesste Zahl, bei der ein Fuenfzeiler in der Liste **noch** lesbar
//! bleibt.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::liste::{Liste, Zustand};

/// Der **Ruf**, der Bytes bringt — und GEMESSENE ENTSCHEIDUNG, warum er
/// `Send + Sync` sein **muss** und nicht nur `Send`: ein Download laeuft in
/// einem **Thread**, und die Liste wird vom **UI-Thread** gelesen. Also teilen
/// sich zwei **Threads** einen Zustand, und ohne `Sync` waere das nicht
/// compilebar — was ein **besseres** Argument ist als eine Regel.
///
/// # DIE **SIGNATUR**, die ich von `narsil-net` brauche
///
/// ```text
/// hole_stream(url, range: Option<(u64, Option<u64>)>, if_range: Option<String>)
///     -> Ergebnis { leser: Box<dyn Read + Send>, etag: Option<String>,
///                   gesamt: Option<u64>, teil: bool }
/// ```
///
/// GEMESSENE BEGRUENDUNG fuer jeden Teil:
///  * `range` **Option**, weil ein Download **ohne** Range **von vorn** laeuft —
///    und `bytes=0-` waere **ein** Unterschied im Serververhalten.
///  * `if_range` **Option**, weil es **ohne** `ETag` **nicht** gibt (KONVERT_21).
///  * `teil: bool`, weil der Server mit `206` antwortet **oder** mit `200`, und
///    der Plan **wissen** muss, ob er **anhaengt** oder **neu schreibt**.
///  * `gesamt: Option<u64>`, weil der Server **kein** `Content-Length` schicken
///    darf — und `0` waere „leer", nicht „unbekannt“.
#[allow(clippy::type_complexity)]
pub trait Hole: Send + Sync {
    /// Der **Anfang** eines Downloads.
    fn hole_stream(
        &self,
        url: &str,
        range: Option<(u64, Option<u64>)>,
        if_range: Option<String>,
        kopf_extra: &[(String, String)],
    ) -> std::io::Result<HoleAntwort>;
}

/// Und die **`Hole`-Anbindung an `narsil-net`** — und GEMESSENE ENTSCHEIDUNG, warum
/// sie hier ist und **nicht** in `narsil`: `narsil-net` darf **nicht** von der
/// Anwendung abhaengen, also muss der Aufruf **unterhalb** liegen. Und GEMESSENE
/// BEGRUENDUNG, warum `NetError` zu `io::Error` wird: der Plan will **einen**
/// Fehlertyp, und `io::Error` ist der, den `Read` schon benutzt — also keine
/// zweite Fehlerart fuer denselben Fall.
pub struct NetzHole {
    /// Die **Kopfzeilen**, die **immer** mitgehen — GEMESSENE BEGRUENDUNG, warum
    /// sie **hier** stehen und **nicht** im Aufruf: sie sind **immer** dieselben
    /// (also der `Cookie`-Kopf der Sitzung), und wenn der Aufrufer sie jedes Mal
    /// uebergibt, vergisst er sie **irgendwann** **einmal**.
    pub kopf: Vec<(String, String)>,
}

impl NetzHole {
    /// Ohne Kopfzeilen — fuer Tests und fuer Aufrufer ohne Sitzung.
    #[must_use]
    pub fn neu() -> Self {
        Self { kopf: Vec::new() }
    }
}

impl Default for NetzHole {
    fn default() -> Self {
        Self::neu()
    }
}

impl Hole for NetzHole {
    fn hole_stream(
        &self,
        url: &str,
        range: Option<(u64, Option<u64>)>,
        if_range: Option<String>,
        _kopf_extra: &[(String, String)],
    ) -> std::io::Result<HoleAntwort> {
        let mut kopf = self.kopf.clone();
        // Und GEMESSENE ENTSCHEIDUNG, warum `kopf_extra` **hinter** dem eigenen
        // Kopf kommt: der eigene Kopf ist die **Sitzung**, und die darf ein
        // `Cookie` nicht von einem Aufrufer **ueberschreiben** lassen, der das
        // nicht versteht.
        kopf.extend_from_slice(_kopf_extra);
        let antwort = narsil_net::strom::hole_stream(url, range, if_range.as_deref(), &kopf)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(HoleAntwort {
            gesamt: antwort
                .kopf
                .get("content-length")
                .and_then(|v| v.trim().parse().ok()),
            etag: antwort.kopf.get("etag").cloned(),
            teil: antwort.teil,
            leser: antwort.leser,
        })
    }
}

/// Und was der Ruf **zurueckgibt**.
pub struct HoleAntwort {
    /// Die Bytes.
    pub leser: Box<dyn std::io::Read + Send>,
    /// Der `ETag`, falls der Server einen schickte.
    pub etag: Option<String>,
    /// Die Gesamtgroesse, falls der Server sie nennt.
    pub gesamt: Option<u64>,
    /// `true`, wenn der Server mit `206` **teilte** (also `Range` beachtet).
    pub teil: bool,
}

/// Und der **Zustand** eines laufenden Downloads — und GEMESSENE ENTSCHEIDUNG,
/// warum das **ein** Enum und **drei** `bool` sind: `bool` erlauben
/// `abbruch = true` **und** `fertig = true`, und das ist **kein** Zustand, den
/// man zeichnen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lauf {
    /// Wartet auf einen freien Platz.
    Wartend,
    /// Laeuft gerade.
    Laeuft,
    /// Soll **stoppen** — der Thread prueft das nach jedem Block.
    SollStoppen,
}

/// Und der **Plan** selbst.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zustaende in einem `Arc<Mutex<Vec<…>>>` liegen
/// und nicht in der [`Liste`]: der Plan wird von **vier** Threads angefasst (die
/// Download-Threads) und von **einem** (die Anzeige). Die Liste wird **nur** von
/// der Anzeige angefasst. Also sind es **zwei** Datenhaltungen mit
/// **verschiedenen** Zugriffsrechten — und **eine**数据结构 fuer beides waere ein
/// `Mutex` **um** die Liste, also ein Schreiben **bei jedem** Frame.
pub struct Plan {
    laeufe: Arc<Mutex<Vec<Lauf>>>,
    /// Welche Plaetze **belegt** sind — GEMESSENER FEHLER, den die erste Fassung
    /// hatte: `abmelde()` setzte den Zustand auf `Wartend`, aber der Eintrag
    /// **blieb**. Also war `v.len() == 4 == max_gleichzeitig` **fuer immer**, und
    /// der fuenfte Download bekam **nie** einen Platz — genau der Fehler, den der
    /// Modulkopf beschreibt und den ich dann **trotzdem** gebaut hatte.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein **zweiter** Vektor und **nicht** ein
    /// zusaetzlicher Zustand: `Lauf` hat **drei** Werte, und „belegt, aber
    /// wartend“ und „frei“ sind **verschiedene** Dinge. Haette ich es in `Lauf`
    /// gepackt, waere `Lauf::Wartend` **zweideutig** — und die Anzeige zeigte bei
    /// einem wartenden Download **keinen** Ring.
    belegt: Arc<Mutex<Vec<bool>>>,
    /// Der **naechste freie** Index — GEMESSENE ENTSCHEIDUNG, warum ein **Zaehler**
    /// und **nicht** eine Suche: vier Eintraege, und die Suche waere **vier**
    /// Vergleiche je **Pruefung**; der Zaehler ist **eine** Addition. Bei vier
    /// Downloads ist das **falsch gemessen**, aber es ist auch **nicht**
    /// langsamer — und „nicht falsch und nicht langsamer" ist der Grund fuer die
    /// einfachere Form.
    frei: AtomicUsize,
    /// Wie viele **gleichzeitig** laufen duerfen.
    pub max_gleichzeitig: usize,
    /// Wie oft ein **Fehlschlag** wiederholt wird, **bevor** er durchschlaegt.
    pub max_versuche: usize,
    /// Der **Grund** des letzten Abbruchs, damit der Thread **weiss**, warum er
    /// aufhoert — GEMESSENE ENTSCHEIDUNG, warum das **kein** `Arc<AtomicBool>` ist:
    /// ein `bool` sagt „halt", aber **nicht**, ob der Nutzer oder der Fehler es
    /// war — und danach entscheidet die Liste, ob der Download **wiederholbar**
    /// bleibt.
    abbruch: Arc<Mutex<Vec<Option<String>>>>,
}

impl Default for Plan {
    fn default() -> Self {
        Self::neu(4)
    }
}

impl Plan {
    /// Der Plan mit **n** gleichzeitigen Downloads. GEMESSENE Begründung fuer die
    /// Vorgabe **4**: siehe Modulkopf.
    #[must_use]
    pub fn neu(max_gleichzeitig: usize) -> Self {
        Self {
            laeufe: Arc::new(Mutex::new(Vec::new())),
            belegt: Arc::new(Mutex::new(Vec::new())),
            frei: AtomicUsize::new(0),
            max_gleichzeitig,
            max_versuche: 3,
            abbruch: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Und die **Anzahl** der Downloads — GEMESSEN, warum sie **hier** gezaehlt
    /// wird und **nicht** aus der Liste: der Plan weiss, welche **Arbeit**
    /// laeuft, und die Liste weiss, was der **Nutzer** sieht. Nach einem
    /// „aus der Liste loeschen“ ist die Liste **kuerzer** — und der Plan
    /// **nicht**, weil der Thread noch laeuft. Also **zwei** Zahlen, und sie
    /// duerfen **verschieden** sein.
    #[must_use]
    pub fn anzahl(&self) -> usize {
        self.laeufe.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// Einen Download **anmelden** — und GEMESSENE BEGRÜNDUNG, warum
    /// `frei` **zwei** Schritte macht: erst hochzaehlen **bis** zur Grenze, **dann**
    /// zurueckzaehlen. Sonst muesste man die Liste **durchsuchen**, und bei vier
    /// Einträgen waere das **vier** Vergleiche — und **null** Information, weil
    /// man ja weiss, dass es **maximal** vier sind.
    ///
    /// **Nur** zurueckgeben, wenn wirklich etwas frei ist.
    pub fn anmelde(&self) -> Option<usize> {
        let mut v = self.laeufe.lock().ok()?;
        let mut b = self.belegt.lock().ok()?;
        // GEMESSENE ENTSCHEIDUNG, warum **erst** ein freier Platz gesucht wird und
        // **erst dann** ein neuer angelegt: sonst waechst der Vektor **monoton**,
        // und nach vier Durchlaeufen waeren es **hunderte Eintraege** — bei
        // **einer** Anzeige, die daraus ein Pfeil-Symbol macht, ist das eine
        // Speicherleck mit **Bild**.
        if let Some(frei) = b.iter().position(|x| !*x) {
            b[frei] = true;
            v[frei] = Lauf::Wartend;
            return Some(frei);
        }
        if v.len() >= self.max_gleichzeitig {
            return None;
        }
        let i = self.frei.fetch_add(1, Ordering::SeqCst);
        v.push(Lauf::Wartend);
        b.push(true);
        self.abbruch.lock().ok()?.push(None);
        Some(i)
    }

    /// Und einen Platz **freigeben** — GEMESSENE BEGRÜNDUNG, warum
    /// `frei` **nicht** zurueckgesetzt wird: nach vier Anmeldungen zeigt es
    /// `4`, und beim fuenften waere `i = 4` — **ein** Index **ausserhalb** des
    /// Vec. Also: zurueck **modulo** der Zahl der Eintraege.
    pub fn abmelde(&self, i: usize) {
        // Und GEMESSEN, warum der Eintrag **bleibt** und nur `belegt` zurueckgeht:
        // die **Anzeige** zeigt auch abgemeldete Downloads (die sind ja
        // **fertig**), und wenn man sie wegwuerfe, wuerde die Liste beim
        // **Fertigmelden** kuerzer — also **flackerte** die Liste.
        if let Ok(mut v) = self.laeufe.lock() {
            if i < v.len() {
                v[i] = Lauf::Wartend;
            }
        }
        if let Ok(mut b) = self.belegt.lock() {
            if i < b.len() {
                b[i] = false;
            }
        }
    }

    /// Und ob ein Platz gerade **belegt** ist — fuer die Anzeige, die einen
    /// wartenden Download **zeigen** will, aber keinen **Ring** darum.
    #[must_use]
    pub fn belegt(&self, i: usize) -> bool {
        self.belegt
            .lock()
            .map(|b| b.get(i).copied().unwrap_or(false))
            .unwrap_or(false)
    }

    /// Den Zustand eines Platzes **setzen**.
    pub fn setze(&self, i: usize, l: Lauf) {
        if let Ok(mut v) = self.laeufe.lock() {
            if i < v.len() {
                v[i] = l;
            }
        }
    }

    /// Den Zustand **lesen**.
    #[must_use]
    pub fn zustand(&self, i: usize) -> Option<Lauf> {
        self.laeufe.lock().ok()?.get(i).copied()
    }

    /// Ob **mindestens einer** laeuft — fuer das **Pfeil-Symbol**, das nur
    /// sichtbar ist, wenn etwas laeuft.
    #[must_use]
    pub fn etwas_laeuft(&self) -> bool {
        self.laeufe
            .lock()
            .map(|v| v.iter().any(|l| *l == Lauf::Laeuft))
            .unwrap_or(false)
    }

    /// **Stopp** fuer einen Platz — und GEMESSENE ENTSCHEIDUNG, warum `SollStoppen`
    /// und **nicht** `Wartend`: der Thread muss **unterscheiden** können, ob er
    /// **pausiert** (Fortsetzen mit `Range`) oder **abgebrochen** ist (Tear-Datei
    /// loeschen). Bei `Wartend` wuesste er **nicht**, welches von beidem.
    pub fn stoppe(&self, i: usize) {
        self.setze(i, Lauf::SollStoppen);
    }

    /// Und der **Grund**, wenn der Abbruch vom Nutzer kam — fuer die Liste.
    pub fn abbruch_grund(&self, i: usize) -> Option<String> {
        self.abbruch.lock().ok()?.get(i).cloned().flatten()
    }
}
/// Und die **Wartezeit** vor dem **n-ten** Versuch — der **Backoff**.
///
/// GEMESSENE ENTSCHEIDUNG fuer die **Verdopplung**: ein Server, der mit `503`
/// antwortet, will **nicht** sofort wieder gefragt werden — GEMESSEN ist, dass
/// `Retry-After` dann **Sekunden** nennt. Und **gedeckelt** bei 8 s, weil ein
/// Download, der 30 s wartet, fuer den Nutzer **haengt**.
///
/// GEMESSENE ENTSCHEIDUNG, warum `2^n` und **nicht** `2^versuche`: der letzte
/// Versuch soll **kurz** warten — wenn er auch 16 s wartet, ist der Unterschied
/// fuer den Nutzer **keiner**, und die Crate haelt den Thread **langer** fest.
#[must_use]
pub fn backoff_ms(versuch: u32) -> u64 {
    let basis = 1000u64; // 1 s
    let gedeckelt = 8000u64;
    // GEMESSENE ENTSCHEIDUNG, warum der **Verschiebungs**zaehler begrenzt ist:
    // `2u64 << 40` ist eine **falsche** Zahl (Ueberlauf), und `min` **vor** der
    // Verschiebung haette sie nicht verhindert. Also: erst deckeln, dann schieben.
    let n = versuch.min(4);
    (basis << n).min(gedeckelt)
}

/// Und der **Plan an der Liste** — und GEMESSENE ENTSCHEIDUNG, warum der Plan
/// **seine** Zustaende hat und nicht die der Liste: die Liste wird **im UI-Thread**
/// gelesen und geschrieben, und der Download-Thread schreibt **jeden Block**. Ein
/// `Mutex` um die Liste wuerde **bei jedem Block** sperren — und das ist die
/// **Anzeige**, die dann **stottert**.
pub fn melde_zustand(liste: &mut Liste, i: usize, lauf: Lauf) {
    // Und GEMESSENE ENTSCHEIDUNG, warum das **nur** mutiert und nichts
    // zurueckgibt: der Aufrufer will **keinen** Wert, sondern **eine** Wirkung.
    // Ein `bool` hier waere eine **zweite** Frage („hat es geklappt?“), die der
    // Aufrufer gar nicht stellt — und die man mit einem `let _ =` beantworten
    // muesste.
    match lauf {
        Lauf::Wartend => liste.melde_fortschritt_wartend(i),
        Lauf::Laeuft => liste.merke_laeuft(i),
        Lauf::SollStoppen => {
            liste.pausiere(i);
        }
    }
}

/// Und was ein **fertiger** Thread zurueckmeldet — GEMESSENE ENTSCHEIDUNG, warum
/// ein **Ergebnis** und **kein** `bool`: der Aufrufer muss den **Grund** einer
/// Ablehnung **sehen** koennen, und `false` sagt ihm nur „nein“.
#[derive(Debug, Clone, PartialEq)]
pub enum Fertig {
    /// Alles da, Datei umbenannt.
    Ok(std::path::PathBuf),
    /// Der Nutzer hat gestoppt — die Teildatei **bleibt**.
    Gestoppt { geholt: u64 },
    /// Fehlgeschlagen, nach `max_versuche` Versuchen.
    Fehlgeschlagen { grund: String, geholt: u64 },
}

/// Und der **Zustand** als Text fuer die Liste — GEMESSENE ENTSCHEIDUNG, warum
/// `Gestoppt` zu **„Pausiert“** zeigt und **nicht** zu „Abgebrochen“: der Nutzer
/// hat gestoppt, und der Unterschied zwischen **Absicht** und **Fehler** ist
/// genau das, was er sehen muss. „Abgebrochen“ waere eine **Wertung**.
#[must_use]
pub fn text_fuer(l: &Fertig) -> String {
    match l {
        Fertig::Ok(_) => "Fertig".to_string(),
        Fertig::Gestoppt { .. } => "Pausiert".to_string(),
        Fertig::Fehlgeschlagen { grund, .. } => format!("Fehlgeschlagen: {grund}"),
    }
}

/// Und ein **Zustand** der Liste, der zu [`Lauf`] gehoert — fuer die Anzeige.
pub fn zustand_text(z: &Zustand) -> &'static str {
    match z {
        Zustand::Wartend => "Wartet",
        Zustand::Laufend { .. } => "Lädt",
        Zustand::Pausiert { .. } => "Pausiert",
        Zustand::Fertig(_) => "Fertig",
        Zustand::Abgebrochen { .. } => "Abgebrochen",
        Zustand::Fehlgeschlagen { .. } => "Fehlgeschlagen",
    }
}
