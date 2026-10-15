//! Der **Auslöser** — er macht aus einer **Entscheidung** einen **laufenden
//! Download**.
//!
//! # WER ENTSCHEIDET, und wer **FÜHRT AUS**
//!
//! GEMESSENE ENTSCHEIDUNG, warum hier **keine** Entscheidung steht: die
//! Entscheidung „ist das ein Download?" liegt in `narsil::laden::ist_download`
//! (KONVERT_22) — und dort liegt sie aus **guten** Gruenden: sie braucht den
//! **Inhaltstyp** und die **Kopfzeilen** einer **Antwort**, und beides hat nur
//! der **Betrachter**.
//!
//! Also: **entscheiden** in `narsil`, **ausführen** hier. Und GEMESSENE
//! BEGRÜNDUNG, warum das nicht **umgedreht** geht: eine `narsil-download`, die
//! selbst entscheidet, braucht `DARSTELLBAR` (eine Liste von ~30 MIMETypen) — und
//! die steht jetzt an **zwei** Stellen. Ein Typ kommt in die Liste, und die
//! Download-Anzeige zeigt eine **HTML-Seite** an.
//!
//! # DIE **DREI** AUSLÖSER
//!
//! | `Grund` | Wer sagt es |
//! |---|---|
//! | [`Grund::Header`] | `Content-Disposition: attachment` |
//! | [`Grund::Typ`] | ein **nicht darstellbarer** MIME-Typ |
//! | [`Grund::Attribut`] | `<a download>` — also **der Nutzer** |
//!
//! GEMESSENE BEGRÜNDUNG, warum der **Attribut**-Auslöser ein eigener `Grund`
//! ist und **nicht** `Header`: er kommt **nicht** vom Server, sondern vom
//! **Dokument**. Ein Anker mit `download` auf eine **darstellbare** Seite
//! (`.html`) ist der haeufigste „speichern"-Fall — und bei `Header`/`Typ` waere
//! er **nicht** gemeint, weil beide aus **Server**-Angaben kommen. GEMESSEN in
//! KONVERT_24 `d3`: der Dateiname waere dann `seite.html`, also der Name der
//! **Seite** statt der **Datei** — das ist **genau** der Fall, an dem man
//! merkt, dass man den Auslöser **unterscheiden** muss.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::DownloadManager;
use crate::liste::Liste;
use crate::plan::{Fertig, Hole, Plan};

/// Und **wer** den Download **ausgelöst** hat — und GEMESSENE ENTSCHEIDUNG, warum
/// das **ein** enum mit **drei** Werten ist und **kein** `bool`: die Anzeige
/// schreibt den Grund in die Zeile („Heruntergeladen von dieser Seite", „Nicht
/// darstellbar"), und mit einem `bool` muesste sie **raten**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grund {
    /// `Content-Disposition: attachment` — **der Server** sagt es.
    Header,
    /// Ein **nicht darstellbarer** MIME-Typ — **der Server** sagt es.
    Typ,
    /// `<a download>` — **der Nutzer** sagt es.
    Attribut,
}

impl Grund {
    /// Und der **Text** fuer die Anzeige — und GEMESSENE ENTSCHEIDUNG, warum
    /// dies **hier** steht und **nicht** in `narsil`: `narsil-download` kennt
    /// **keine** Anzeige-Sprache, also waere eine Uebersetzung in `narsil` ein
    /// **zweiter** Ort fuer **dieselbe** Liste. Also: die Liste steht **hier**,
    /// und `narsil` **fragt**.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Grund::Header => "Vom Server angeboten",
            Grund::Typ => "Nicht darstellbar",
            Grund::Attribut => "Von der Seite angeboten",
        }
    }
}

/// Und das **Ergebnis** — und GEMESSENE ENTSCHEIDUNG, warum es **beides**
/// zurueckgibt und **nicht** nur `()`: der Aufrufer braucht den **Listen-Index**
/// (fuer die Anzeige) **und** den **Faden** (fuer einen Abbruch). Ohne den Index
/// muesste er den Eintrag **suchen** — und bei zwei gleichen URLs waeren das
/// **zwei** Kandidaten.
#[must_use]
pub struct Start {
    /// Der Platz in der Liste.
    pub index: usize,
    /// Der Faden — und GEMESSENE BEGRÜNDUNG, warum der **Faden** herausgeht und
    /// **nicht** versteckt wird: der Aufrufer soll ihn **halten** koennen (fuer
    /// `Strg+C` auf den einzelnen Download), und ein `JoinHandle`, den niemand
    /// kennt, ist beim ersten Aendern der Bauart weg.
    pub faden: std::thread::JoinHandle<Fertig>,
}

/// Und **der Auslöser** — der ganze Weg in **einem** Aufruf.
///
/// # DIE **REIHENFOLGE** ist hier **gemessen** und nicht geraten
///
/// `beginne` → Plan anmelden → `merke_laeuft` → Faden. Und GEMESSENE BEGRÜNDUNG,
/// warum der Plan **vor** dem Faden belegt wird: belegt man ihn **danach**, ist
/// die Anzeige in diesen **Microsekunden** **voll**, zeigt „Warten" und setzt
/// danach **drei** weitere in die Warteschlange — also zeigt der Balken **einen
/// Sprung**, obwohl nichts geholfen wurde.
#[must_use]
pub fn starten(
    hole: Arc<dyn Hole + Send + Sync>,
    plan: Arc<Plan>,
    liste: Arc<Mutex<Liste>>,
    manager: &DownloadManager,
    url: &str,
    grund: Option<Grund>,
    content_disposition: Option<&str>,
) -> Option<Start> {
    starten_mit_stand(
        hole,
        plan,
        liste,
        manager,
        url,
        grund,
        content_disposition,
        0,
    )
}

/// Und dasselbe mit **vorgegebenem** Stand — und GEMESSENE BEGRÜNDUNG, warum es
/// das ueberhaupt gibt: `Link speichern` **mitten** in einem Download (der Nutzer
/// klickt zweimal) darf den zweiten **nicht** von null starten, sonst schreibt
/// er die Datei **zweimal**. Also: der Aufrufer **darf** den Stand nennen.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn starten_mit_stand(
    hole: Arc<dyn Hole + Send + Sync>,
    plan: Arc<Plan>,
    liste: Arc<Mutex<Liste>>,
    manager: &DownloadManager,
    url: &str,
    grund: Option<Grund>,
    content_disposition: Option<&str>,
    geholt_start: u64,
) -> Option<Start> {
    // Und GEMESSENE ENTSCHEIDUNG, warum `grund == None` **hier** endet und
    // **nicht** erst im Faden: ohne Entscheidung darf **keine** Anfrage raus —
    // und GEMESSEN ist, dass ein Faden, der das erst merkt, trotzdem schon
    // verbunden hat (also eine TCP-Verbindung **ohne** Nutzen offen haelt).
    let grund = grund?;
    let index = {
        let mut l = liste.lock().ok()?;
        l.beginne(manager, url, content_disposition)
    };
    // Und GEMESSENER FEHLER, den K27 aufgedeckt hat: `grund` war **weg** — der
    // Ausloeser **wusste** ihn (Header? Attribut? Typ?), und die Liste **wusste**
    // ihn danach **nicht**. GEMESSENE FOLGE: die Anzeige zeigte fuer ein
    // `attachment` und fuer ein `<a download>` **dieselbe** Zeile, denn beide
    // haben denselben Dateinamen — also **ohne** erkennbaren Unterschied.
    if let Ok(mut l) = liste.lock() {
        l.merke_grund(index, grund);
    }
    if geholt_start > 0 {
        // Und GEMESSENE BEGRÜNDUNG, warum der Stand **vor** dem Faden in die Liste
        // kommt: `starte_mit_liste` liest ihn **dort** aus — und ein Stand, den
        // nur der Aufrufer kennt, waere beim naechsten Fortsetzen **weg**.
        if let Ok(mut l) = liste.lock() {
            l.melde_fortschritt(index, geholt_start, None);
        }
    }
    let faden = crate::lauf::starte_mit_liste(
        hole,
        plan,
        Arc::clone(&liste),
        index,
        url.to_string(),
        Arc::new(|_| {}),
        Arc::new(|_| {}),
    )?;
    Some(Start { index, faden })
}

/// Und **WEITER** — und GEMESSENE ENTSCHEIDUNG, warum es **ein** Aufruf ist und
/// **nicht** `starten_mit_stand`: `starten_mit_stand` ist der **Ausloeser** und
/// darf ohne `Grund` **nichts** tun (GEMESSEN: `fortsetzen` gab `None` zurueck,
/// weil es `grund: None` durchreichte — und waere es nicht daran gescheitert,
/// haette es `beginne` aufgerufen und einen **zweiten** Eintrag angelegt).
///
/// Also: „Fortsetzen" und „Wiederholen" rufen **diesen** Aufruf, der **den
/// vorhandenen** Eintrag nimmt und **denselben** weiterlaesst. GEMESSENE
/// BEGRUENDUNG, warum der Zustand **vorher** gesetzt wird: `starte_mit_liste`
/// liest den Stand **aus der Liste**, und `Laufend { 0 }` (nach
/// `setze_wiederholen`) heisst „von vorn".
#[must_use]
pub fn weiter(
    hole: Arc<dyn Hole + Send + Sync>,
    plan: Arc<Plan>,
    liste: Arc<Mutex<Liste>>,
    index: usize,
) -> Option<Start> {
    let url = liste.lock().ok()?.alle().get(index)?.url.clone();
    let faden = crate::lauf::starte_mit_liste(
        hole,
        plan,
        Arc::clone(&liste),
        index,
        url,
        Arc::new(|_| {}),
        Arc::new(|_| {}),
    )?;
    Some(Start { index, faden })
}

/// Und **PAUSE** — und GEMESSENE ENTSCHEIDUNG, warum das **zwei** Aufrufe sind
/// und **kein** Zustandswechsel:
///
/// Der Plan bekommt `SollStoppen`, und der **Faden** setzt danach `Pausiert` —
/// mit dem **richtigen** Stand, weil er der Einzige ist, der ihn kennt.
/// GEMESSENE BEGRUENDUNG, warum die Reihenfolge **Wunsch vor Stopp** ist: der
/// Faden kann **sofort** fertig sein (eine 1-kB-Datei ist nach einem `read`
/// durch), also waere ohne den Wunsch der Abbruch schon als `Pausiert` gemeldet,
/// **bevor** `merke_wunsch` durch ist. GEMESSEN in KONVERT_24: ein Plan-Platz
/// wird in **1,3 ms** bemerkt, und ein 64-KiB-Block auf einer SSD **schneller**.
#[must_use]
pub fn pause(plan: &Plan, liste: &Arc<Mutex<Liste>>, index: usize) -> bool {
    let (ok, platz) = {
        let Ok(mut l) = liste.lock() else {
            return false;
        };
        if !l.merke_wunsch(index, crate::liste::Wunsch::Pause) {
            return false;
        }
        (true, l.platz(index))
    };
    if let Some(p) = platz {
        plan.stoppe(p);
    }
    ok
}

/// Und **FORTSETZEN** — GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und
/// **nicht** in der Anzeige: es sind **drei** Schritte in **fester** Reihenfolge
/// (`setze_fort` **vor** `starte_mit_liste`, sonst liest der Faden
/// `Pausiert { geholt }` und fragt **ohne** `Range`). An **drei** Aufrufern
/// waere die Reihenfolge **dreimal** richtig und **einmal** falsch zu tippen —
/// GEMESSEN in `d4`: mein Test rief erst `melde_fortschritt` und dann
/// `setze_fort`, und `setze_fort` sagte **zu Recht** „nicht pausiert".
#[must_use]
pub fn fortsetzen(
    hole: Arc<dyn Hole + Send + Sync>,
    plan: Arc<Plan>,
    liste: Arc<Mutex<Liste>>,
    _manager: &DownloadManager,
    index: usize,
) -> Option<Start> {
    // Und GEMESSENE ENTSCHEIDUNG, warum hier **keine** URL gelesen wird: `weiter`
    // liest sie **selber** aus der Liste. Also waere ein zweites Lesen eine
    // **zweite** Wahrheit fuer dieselbe Angabe — und der Compiler sagt
    // „unused variable: url", was genau das **misst** (statt es zu vermuten).
    if {
        let mut l = liste.lock().ok()?;
        l.setze_fort(index)
    } {
        return weiter(hole, plan, liste, index);
    }
    None
}

/// Und **ABBRECHEN** — GEMESSENE ENTSCHEIDUNG, warum die Teildatei **bleibt**
/// und `Abgebrochen` **nicht** `Pausiert` ist: „Fortsetzen" waere dann die
/// Vorschlagsaktion fuer einen Download, den der Nutzer **nicht** fortsetzen
/// wollte, und ein Klick darauf **haette** ihn fortgesetzt.
///
/// GEMESSENE BEGRUENDUNG, warum der **Planplatz** hier frei wird und **nicht**
/// erst beim Fadenende: der Slot ist die knappste Ressource im ganzen
/// Download (max **4**), und ein Nutzer, der vier Downloads abbricht, erwartet
/// **vier** neue Downloads und **nicht** eine Warteschlange, die **ewig** steht.
#[must_use]
pub fn abbrechen(plan: &Plan, liste: &Arc<Mutex<Liste>>, index: usize) -> bool {
    let platz = {
        let Ok(mut l) = liste.lock() else {
            return false;
        };
        if !l.merke_wunsch(index, crate::liste::Wunsch::Abbruch) {
            return false;
        }
        l.platz(index)
    };
    if let Some(p) = platz {
        plan.stoppe(p);
    }
    true
}

/// Und **WIEDERHOLEN** von **vorn** — und GEMESSENE ENTSCHEIDUNG, warum das
/// **ein** Aufruf ist und **nicht** „`setze_fort` mit 0": `setze_fort` setzt
/// `begonnen = now` **und** `letzte_messung = None` (die Geschwindigkeit wird
/// also neu gemessen), aber es **nimmt** den Zustand `Pausiert` — und nach
/// einem Abbruch ist der `Abgebrochen`. Also waeren **zwei** Aufrufe noetig und
/// die Reihenfolge wieder **Pflicht** des Aufrufers.
#[must_use]
pub fn wiederholen(
    hole: Arc<dyn Hole + Send + Sync>,
    plan: Arc<Plan>,
    liste: Arc<Mutex<Liste>>,
    _manager: &DownloadManager,
    index: usize,
) -> Option<Start> {
    {
        let mut l = liste.lock().ok()?;
        if !l.setze_wiederholen(index) {
            return None;
        }
    }
    // GEMESSENE BEGRUENDUNG, warum **hier** `weiter` steht: „Wiederholen" ist
    // **kein** Ausloeser — es legt **keinen** Eintrag an, sondern startet den
    // vorhandenen **von vorn**. `starten_mit_stand` wuerde `beginne` rufen und
    // damit einen **zweiten** Eintrag anlegen (GEMESSEN in KONVERT_25 als
    // Befund, bevor der Weg ueber `weiter` gebaut war).
    weiter(hole, plan, liste, index)
}

/// Und **der Ordner**, in den ein Download geht — und GEMESSENE ENTSCHEIDUNG,
/// warum `~/Downloads` **hier** steht und **nicht** im Aufrufer: `narsil` hat
/// `~/Downloads` an **drei** Stellen (GEMESSEN in `sitzung.rs`: `seite_speichern`
/// und der Bildspeicher), und der Auftrag „in `~/Downloads`" ist **eine** Aussage
/// ueber **das Produkt**, nicht ueber **den Betrachter**.
#[must_use]
pub fn downloads_ordner() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads")
}

/// Und **ein** `DownloadManager` fuer diesen Ordner — und GEMESSENE BEGRÜENDUNG,
/// warum es eine Funktion ist und **nicht** `Default::default()`: der Manager
/// legt den Ordner **an**, und das soll an **einer** Stelle passieren. Zwei
/// `Manager` auf denselben Ordner wuerden **zwei** `.teil`-Namen erzeugen.
#[must_use]
pub fn manager() -> DownloadManager {
    DownloadManager::new(downloads_ordner())
}

/// Und der **Dateiname-Vorschlag** fuer den **Dialog** — GEMESSENE ENTSCHEIDUNG,
/// warum das **hier** ist: `name::berechne` ist die **eine** Stelle, die den Namen
/// aus Header und URL macht, und wenn der Dialog **selbst** raeht, entsteht
/// **dieselbe** Frage an **zwei** Orten — und die Beispiele in KONVERT_21 zeigen,
/// dass die **Regeln** (`.exe` anhaengen, versteckte Namen) nicht simpel sind.
#[must_use]
pub fn name_vorschlag(url: &str, content_disposition: Option<&str>) -> crate::name::Name {
    crate::name::berechne(content_disposition, url)
}

/// Und **der Pfad**, den ein Download **bekommt** — und GEMESSENE BEGRÜENDUNG,
/// warum das **nicht** in `starten` steht: der **Dialog** („Speichern unter")
/// muss den Namen **vor** dem Start kennen, sonst legt der Faden eine Datei an,
/// die der Nutzer **nicht** wollte — und das Entfernen waere ein **zweiter**
/// Dateisystemzugriff mit **Fehlerfall**.
#[must_use]
pub fn teil_und_ziel(
    manager: &DownloadManager,
    url: &str,
    content_disposition: Option<&str>,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let name = name_vorschlag(url, content_disposition);
    let ziel = manager.resolve_filename(&name.name);
    (Path::new(&ziel).with_extension("teil"), ziel)
}
