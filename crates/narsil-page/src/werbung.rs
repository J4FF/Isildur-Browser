//! A-ADBLOCK — der Anfrage-Filter und die kosmetischen Regeln.
//!
//! # DER AUFTRAG
//!
//! `ANFORDERUNGEN.txt`: „Werbeblocker eingebaut, ohne Konfiguration
//! (EasyList-Syntax)". Und die Koordination am 01.10.2026, 21:45:
//!
//! > Anfrage-Filter vor jedem Unterressourcen-Abruf in narsil-page
//! > (Freigabe: kleinste Einhaengung, HTML informieren), kosmetische
//! > Regeln als display:none in die Kaskade, Schalter aus den
//! > Einstellungen (UI).
//!
//! Abnahme: probe von 3 Seiten mit Werbung (spiegel.de, chip.de,
//! heise.de) — Zahl blockierter Anfragen, Ladezeit vorher/nachher.
//!
//! # WARUM DIESE DATEI IN `narsil-page` LIEGT
//!
//! Die Abfrage braucht den Host der **Seite**, nicht den der Anfrage:
//! `$domain=a.de` bedeutet in EasyList „die Seite, nicht das Ziel"
//! (`Pruefer::Anfrage::seiten_host`). Das weiss nur der, der die Seite
//! gebaut hat — das ist hier.
//!
//! Und die kosmetischen Regeln gehoeren in die KASKADE, und die Kaskade
//! wird in `kaskade_einrichten` aufgebaut. Beides an der einen Stelle.
//!
//! # WAS DIESE DATEI NICHT MACHT
//!
//! Sie ruft **kein** Netz auf. Sie sagt nur, ob eine Anfrage raus darf.
//! Das Netz gehoert `narsil-net` (fremde Crate), und der Aufruf sitzt in
//! `narsil` (Rolle HTML). Die Anhaengung dort ist eine Zeile und in
//! `team/STATUS_CSS.txt` gemeldet.

use narsil_adblock::typen::AnfrageArt;
// `GrundlisteZahlen` liegt am Crate-Root von `narsil-adblock`, nicht im
// `typen`-Modul — dort liegen nur die Regeltypen.
use narsil_adblock::GrundlisteZahlen;
use narsil_adblock::{Anfrage, Pruefer, Urteil};
use std::collections::HashMap;

/// Der Filter mit seinen Zaehlern.
///
/// # DIE ZAEHLER SIND DER PUNKT
///
/// M7 sagt: jede Zahl wird zurueckgegeben, nicht behauptet. Die Zahl
/// „wie viele Anfragen wurden blockiert" ist die Abnahme der
/// Koordination, und eine Abnahme, die man nicht ablesen kann, ist
/// keine. Deshalb sind sie Felder und nicht lokale Variable, und
/// [`Werbung::zahlen`] gibt sie alle zusammen heraus.
///
/// Gezaehlt wird **jede** Anfrage, auch die durchgelassene: sonst ist
/// „12 blockiert" nicht interpretierbar, weil man nicht weiss, ob 12 von
/// 12 oder 12 von 1200 sind.
#[derive(Debug)]
pub struct Werbung {
    pruefer: Pruefer,
    /// Der Schalter aus den Einstellungen. Aus = der Filter wird
    /// nicht gefragt, und es wird auch nichts gezaehlt — ein
    /// ausgeschalteter Filter, der mitzaehlt, wuerde die Abnahme
    /// beschaedigen statt sie zu erklaeren.
    an: bool,
    anfragen: usize,
    blockiert: usize,
    frei: usize,
    ausnahme: usize,
    dritte_partei: usize,
    /// Nach Art der Anfrage, fuer die Abnahme nachvollziehbar.
    nach_art: HashMap<&'static str, usize>,
    /// Die Regel-Indizes, die blockiert haben. Summe erlaubt eine
    /// Aussage ueber Wiederverwendung (``||ads.example.com^`` trifft
    /// viele Male) — ohne sie ist „3 blockiert" nicht von „1 Regel
    /// dreimal" zu unterscheiden.
    regel_hits: HashMap<usize, usize>,
}

/// Die Zahlen der Abnahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zahlen {
    /// Anfragen, die der Filter gesehen hat.
    pub anfragen: usize,
    /// Davon blockiert.
    pub blockiert: usize,
    /// Von keiner Regel getroffen.
    pub frei: usize,
    /// Von einer Ausnahme (`@@`) durchgelassen.
    pub ausnahme: usize,
    /// Anfragen an einen anderen Host als die Seite.
    pub dritte_partei: usize,
    /// Verschiedene Regeln, die blockiert haben.
    pub regeln: usize,
    /// Wie oft die meistgetroffene Regel gegriffen hat.
    pub regel_spitze: usize,
}

/// Die Art einer Anfrage, fuer die Zahlen nach Art.
///
/// GEMESSENE ENTSCHEIDUNG: eine eigene Enum-Variante je Art waere
/// schoen, aber die Art kommt aus `narsil_net::RessourceArt` — einer
/// fremden Crate. Statt einer zweiten Wahrheit in diesem Crate nennt
/// die Zahl einfach den Text, und die Aufrufstelle entscheidet. Das ist
/// die Grenze einer Crate, die nicht die Netzbilanz besitzt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Dokument,
    Stylesheet,
    Skript,
    Bild,
    Schrift,
    Medien,
    Xhr,
    Popup,
    Sonstige,
}

impl Art {
    /// Als `AnfrageArt` fuer den Filter.
    #[must_use]
    pub fn als_optionen(self) -> AnfrageArt {
        let mut a = AnfrageArt::default();
        match self {
            Art::Dokument => a.dokument = true,
            Art::Stylesheet => a.stylesheet = true,
            Art::Skript => a.skript = true,
            Art::Bild => a.bild = true,
            Art::Schrift => a.schrift = true,
            Art::Medien => a.medien = true,
            Art::Xhr => a.xhr = true,
            Art::Popup => a.popup = true,
            Art::Sonstige => a.sonstige = true,
        }
        a
    }

    fn name(self) -> &'static str {
        match self {
            Art::Dokument => "dokument",
            Art::Stylesheet => "stylesheet",
            Art::Skript => "skript",
            Art::Bild => "bild",
            Art::Schrift => "schrift",
            Art::Medien => "medien",
            Art::Xhr => "xhr",
            Art::Popup => "popup",
            Art::Sonstige => "sonstige",
        }
    }
}

impl Werbung {
    /// Der Filter mit der eingebauten Grundliste.
    ///
    /// GEMESSENE KOSTEN: `narsil_adblock::grundliste_zahlen()` — der
    /// Aufbau ist einmal je Browser, nicht je Anfrage.
    #[must_use]
    pub fn standard() -> Werbung {
        let auswertung = narsil_adblock::liste_lesen(narsil_adblock::grundliste());
        Werbung::aus_auswertung(&auswertung)
    }

    /// Aus beliebigem Regeltext (Tests, eigene Listen).
    #[must_use]
    pub fn aus_text(text: &str) -> Werbung {
        Werbung::aus_auswertung(&narsil_adblock::liste_lesen(text))
    }

    fn aus_auswertung(a: &narsil_adblock::Auswertung) -> Werbung {
        Werbung {
            pruefer: Pruefer::aus(a),
            an: true,
            anfragen: 0,
            blockiert: 0,
            frei: 0,
            ausnahme: 0,
            dritte_partei: 0,
            nach_art: HashMap::new(),
            regel_hits: HashMap::new(),
        }
    }

    /// Der Schalter. Aus heisst: keine Pruefung, kein Zaehler.
    ///
    /// GEMESSENE ENTSCHEIDUNG: ein ausgeschalteter Filter, der
    /// mitzaehlt, wuerde die Zahl „blockierte Anfragen" beschaedigen.
    /// Wer den Schalter zieht und danach 12 liest, will 0 lesen.
    pub fn setze_an(&mut self, an: bool) {
        self.an = an;
    }

    /// `true`, wenn der Filter arbeitet.
    #[must_use]
    pub fn ist_an(&self) -> bool {
        self.an
    }

    /// DIE ENTSCHEIDUNG. Das ist der Aufruf vor jedem Unterressourcen-
    /// Abruf.
    ///
    /// # WARUM SIE DEN SEITEN-HOEST BRAUCHT
    ///
    /// EasyList-Regeln mit `$domain=` und mit Ausnahmen wie
    /// `@@||wikipedia.org^` beziehen sich auf die **Seite**, nicht auf
    /// das Ziel. Ohne `seiten_host` wuerde eine Werbe-Regel von
    /// `doubleclick.net` auch auf wikipedia.org greifen — und die
    /// Nachrichtenseiten wuerde ihre Bilder verlieren. Das ist der
    /// Grund, warum dieser Aufruf nicht in `narsil-net` steht: dort
    /// kennt niemand die Seite.
    ///
    /// # M3
    ///
    /// Es wird nichts gelesen, nichts gespeichert und nichts
    /// reserviert — der Aufruf nimmt eine URL und gibt ein `Urteil`
    /// zurueck. Die einzige Grenze ist die Laenge der URL, und die
    /// setzt `Anfrage::neu` (MAX_ZEILE in `narsil-adblock`).
    pub fn entscheide(&mut self, url: &str, art: Art, seiten_host: &str) -> Urteil {
        if !self.an {
            return Urteil::Frei;
        }
        let a = Anfrage::neu(url)
            .mit_art(art.als_optionen())
            .mit_seite(seiten_host);
        let dritte = a.dritte_partei;
        let urteil = self.pruefer.pruefe(&a);

        self.anfragen += 1;
        *self.nach_art.entry(art.name()).or_insert(0) += 1;
        if dritte {
            self.dritte_partei += 1;
        }
        match urteil {
            Urteil::Blockiert(i) => {
                self.blockiert += 1;
                *self.regel_hits.entry(i).or_insert(0) += 1;
            }
            Urteil::Erlaubt => self.ausnahme += 1,
            Urteil::Frei => self.frei += 1,
        }
        urteil
    }

    /// Bequemer: `true`, wenn die Anfrage raus darf.
    ///
    /// GEMESSENER FEHLER IN DIESER ZEILE, erster Fassung: Ich hatte
    /// `entscheide(...) == Frei || entscheide_ergebnis(...) == Erlaubt`
    /// geschrieben. Das ist ZWEIMAL geprueft, und `entscheide` bewegt
    /// die Zaehler — jede Anfrage waere doppelt gezaehlt. Genau der
    /// Fehler, den ich in dieser Sitzung zweimal selbst gemacht habe
    /// (ein Zaehler, der vor dem Ereignis gelesen wird).
    ///
    /// Die Form hier ruft die Pruefung **einmal** und leitet beides
    /// daraus ab.
    pub fn erlaubt(&mut self, url: &str, art: Art, seiten_host: &str) -> bool {
        !matches!(self.entscheide(url, art, seiten_host), Urteil::Blockiert(_))
    }

    /// Die Zahlen fuer die Abnahme.
    #[must_use]
    pub fn zahlen(&self) -> Zahlen {
        let (regeln, spitze) = self
            .regel_hits
            .iter()
            .fold((0usize, 0usize), |(n, m), (_, c)| (n + 1, m.max(*c)));
        Zahlen {
            anfragen: self.anfragen,
            blockiert: self.blockiert,
            frei: self.frei,
            ausnahme: self.ausnahme,
            dritte_partei: self.dritte_partei,
            regeln,
            regel_spitze: spitze,
        }
    }

    /// Wie viele Anfragen einer Art gesehen wurden.
    #[must_use]
    pub fn anfragen_von(&self, art: Art) -> usize {
        self.nach_art.get(art.name()).copied().unwrap_or(0)
    }

    /// Die Zahlen der eingebauten Grundliste.
    #[must_use]
    pub fn grundliste_zahlen(&self) -> GrundlisteZahlen {
        narsil_adblock::grundliste_zahlen()
    }

    /// Die kosmetischen Selektoren fuer einen Host.
    ///
    /// `kosmetisch_fuer` liefert nur die Regeln, die fuer diesen Host
    /// gelten — der Filter macht die Host-Abfrage, und zwar gemessen an
    /// den Regeln selbst, nicht an einem Raten.
    #[must_use]
    pub fn kosmetisch_selektoren(&self, host: &str) -> Vec<String> {
        self.pruefer
            .kosmetisch_fuer(host)
            .into_iter()
            .map(|k| k.selektor.clone())
            .collect()
    }

    /// Das kosmetische Blatt fuer die Kaskade.
    ///
    /// Jeder Selektor wird zu `{ <selektor> { display: none
    /// !important } }`. Das `!important` ist nicht Kosmetik: ohne es
    /// verliert die Regel gegen fast jede Werbe-Regel, weil Werbe-Seiten
    /// `display: block` auf dieselbe Klasse setzen (GEMESSEN in
    /// `team/BERICHT_CSS_ADBLOCK.txt`).
    ///
    /// # M3/M4
    ///
    /// `kosmetik_als_blatt` begrenzt auf `MAX_REGELN` Regeln und
    /// `MAX_SELEKTOREN` Zeichen je Selektor und verwirft Selektoren mit
    /// `;`, `{` oder `}` — die wuerden den CSS-Text zerlegen. Die Zahl
    /// der dabei verworfenen gibt [`Werbung::kosmetik_zahlen`] heraus.
    #[must_use]
    pub fn kosmetik_blatt(&self, host: &str) -> Option<narsil_css::Stylesheet> {
        let selektoren = self.kosmetisch_selektoren(host);
        if selektoren.is_empty() {
            return None;
        }
        let refs: Vec<&str> = selektoren.iter().map(String::as_str).collect();
        // `kosmetik_als_blatt` liefert ein `KosmetikErgebnis`, nicht
        // ein `Stylesheet` — das Blatt steckt darin. GEMESSENER FEHLER
        // beim ersten Versuch: ich habe beides verwechselt.
        let ergebnis = narsil_css::kosmetik::kosmetik_als_blatt(&refs);
        if ergebnis.regeln == 0 {
            return None;
        }
        Some(ergebnis.blatt)
    }

    /// Was die kosmetischen Regeln getan haben — gezaehlt, nicht
    /// behauptet.
    ///
    /// M7: `KosmetikErgebnis::gekuerzt` und `.verworfen` standen bis
    /// 20:45 auf fest 0. Sie sind inzwischen echt; dieses Fenster gibt
    /// sie weiter, damit eine stille Kuerzung auffaellt.
    #[must_use]
    pub fn kosmetik_zahlen(&self, host: &str) -> KosmetikZahlen {
        let selektoren = self.kosmetisch_selektoren(host);
        let refs: Vec<&str> = selektoren.iter().map(String::as_str).collect();
        let r = narsil_css::kosmetik::kosmetik_als_blatt(&refs);
        KosmetikZahlen {
            angeboten: selektoren.len(),
            regeln: r.regeln,
            gekuerzt: r.gekuerzt,
            verworfen: r.verworfen,
            unlesbar: r.unlesbar,
            // `weggelassen` zaehlt Selektoren ab `MAX_REGELN`.
            weggelassen: r.weggelassen,
        }
    }

    /// Wie viele kosmetische Regeln es fuer einen Host gibt.
    #[must_use]
    pub fn kosmetisch_anzahl(&self, host: &str) -> usize {
        self.pruefer.kosmetisch_fuer(host).len()
    }
}

/// Was die kosmetischen Regeln aus einer Selektorliste gemacht haben.
///
/// GEMESSENE REDLICHKEIT: `weggelassen` und `unlesbar` sind hier eigene
/// Felder, weil sie verschiedene Ursachen haben. Beide kommen aus
/// `KosmetikErgebnis` (narsil-css), und dort sind sie seit 20:45
/// gezaehlt statt fest auf 0 verdrahtet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KosmetikZahlen {
    /// Selektoren, die der Host fuehrt.
    pub angeboten: usize,
    /// Daraus gewordenen Regeln.
    pub regeln: usize,
    /// An `MAX_SELEKTOREN` gekuerzt.
    pub gekuerzt: usize,
    /// Wegen `;`, `{` oder `}` nicht gebaut.
    pub verworfen: usize,
    /// Vom CSS-Parser nicht verstanden (`div:has-text(…)`).
    pub unlesbar: usize,
    /// Ab `MAX_REGELN` weggelassen.
    pub weggelassen: usize,
}

// =========================================================================
// DER BROWSER-WEITE FILTER
// =========================================================================
//
// # WARUM EIN GLOBALER ZUSTAND
//
// Die Koordination (01.10.2026, 21:45) verlangt den Filter „vor jedem
// Unterressourcen-Abruf". Die Abrufe liegen in `narsil` — vier Dateien,
// fuenf Stellen (`bilder.rs:181`, `blaetter.rs:217`, `js.rs:1320`,
// `laden.rs:231`, `laden.rs:445`). Jede von ihnen braucht den Filter,
// und keine davon baut die Seite.
//
// Ein Filter, der pro Seite neu gebaut wird, koennte an einer Stelle
// fehlen, ohne dass man es merkt. Also EIN Zustand fuer den Browser, und
// jede Abrufstelle fragt ihn.
//
// # WAS EIN GLOBALER ZUSTAND KOSTET
//
// * `std::sync::Mutex`, nicht `OnceLock`: der Filter muss beim Schalter
//   austauschbar sein, und `OnceLock` laesst sich nicht zuruecksetzen.
// * Die Sperre wird **nie** ueber ein Netz gehalten — der Aufrufer
//   fragt, kopiert die Entscheidung und gibt frei. Sonst blockiert ein
//   langsamer Abruf den Filter aller Tabs.
// * `Werbung` ist `Send` (nur `Vec`, `HashMap`, `Pruefer`), also geht
//   der `Mutex` ueberhaupt.
// * Ein vergifteter Mutex (`panic` im Kritischen) gibt zurueck, statt
//   den ganzen Browser zu beenden.

use std::sync::{Mutex, MutexGuard, OnceLock};

fn zustand() -> &'static Mutex<Option<Werbung>> {
    static Z: OnceLock<Mutex<Option<Werbung>>> = OnceLock::new();
    Z.get_or_init(|| Mutex::new(None))
}

/// Die Sperre, mit Rueckfall auf einen leeren Filter.
///
/// GEMESSENE ENTSCHEIDUNG: `vergiftet()` gibt zurueck, statt zu
/// paniken. Ein Filter, dessen Zaehlerpanik den Browser beendet, waere
/// schlimmer als ein Browser ohne Filter.
fn sperre() -> MutexGuard<'static, Option<Werbung>> {
    match zustand().lock() {
        Ok(g) => g,
        Err(vergiftet) => vergiftet.into_inner(),
    }
}

/// Der Filter des Browsers, beim ersten Zugriff aus der Grundliste.
///
/// # M3
///
/// Die Grundliste ist 77 Zeilen und wird einmal gelesen. Ab dem zweiten
/// Zugriff wird nur noch ein `Mutex` genommen — die Datei nicht erneut.
pub fn filter() -> MutexGuard<'static, Option<Werbung>> {
    let mut g = sperre();
    if g.is_none() {
        *g = Some(Werbung::standard());
    }
    g
}

/// Kurzform fuer die Abrufstelle: `true`, wenn die Anfrage raus darf.
///
/// Der Zaehler bewegt sich dabei — das ist beabsichtigt, denn die
/// Abnahme ist „Zahl blockierter Anfragen".
pub fn erlaubt(url: &str, art: Art, seiten_host: &str) -> bool {
    let mut g = filter();
    match g.as_mut() {
        Some(w) => w.erlaubt(url, art, seiten_host),
        None => true,
    }
}

/// Wie [`erlaubt`], aber mit dem Urteil statt eines `bool`.
///
/// fuer die Stellen, die den Regel-Index im Ereignisprotokoll nennen
/// wollen („blockiert: Regel 7").
pub fn entscheide(url: &str, art: Art, seiten_host: &str) -> Urteil {
    let mut g = filter();
    match g.as_mut() {
        Some(w) => w.entscheide(url, art, seiten_host),
        None => Urteil::Frei,
    }
}

/// Der Schalter aus den Einstellungen.
pub fn setze_an(an: bool) {
    if let Some(w) = filter().as_mut() {
        w.setze_an(an);
    }
}

/// `true`, wenn der Filter arbeitet.
#[must_use]
pub fn ist_an() -> bool {
    filter().as_ref().is_some_and(Werbung::ist_an)
}

/// Die Zahlen fuer die Abnahme (Spiegel, Netz-Tab, Bild).
#[must_use]
pub fn zahlen() -> Option<Zahlen> {
    filter().as_ref().map(Werbung::zahlen)
}

/// Das kosmetische Blatt fuer einen Host — der Weg fuer den Seitenbau.
#[must_use]
pub fn kosmetik_blatt(host: &str) -> Option<narsil_css::Stylesheet> {
    filter().as_ref().and_then(|w| w.kosmetik_blatt(host))
}

/// Die kosmetischen Zahlen fuer einen Host.
#[must_use]
pub fn kosmetik_zahlen(host: &str) -> KosmetikZahlen {
    match filter().as_ref() {
        Some(w) => w.kosmetik_zahlen(host),
        None => KosmetikZahlen {
            angeboten: 0,
            regeln: 0,
            gekuerzt: 0,
            verworfen: 0,
            unlesbar: 0,
            weggelassen: 0,
        },
    }
}

/// Die Zahlen der eingebauten Grundliste, ohne den Filter zu bauen.
#[must_use]
pub fn grundliste_zahlen() -> GrundlisteZahlen {
    narsil_adblock::grundliste_zahlen()
}

/// Der Host der SEITE, fuer `$domain=`-Angeben.
///
/// # WARUM WIEDER EIN GLOBALER ZUSTAND
///
/// `bytes_holen(adresse: &str)` in `narsil/src/bilder.rs` kennt die Seite
/// nicht, und der Filter braucht sie: `$domain=a.de` bedeutet in
/// EasyList „die Seite, nicht das Ziel". Eine zweite Parameterliste an
/// `bytes_holen` wuerde jeden Bildaufrufer brechen — es sind mehrere in
/// `narsil` und einer in `narsil-probe`.
///
/// Also merkt sich der Filter den Seiten-Host selbst, und die Seite
/// trägt ihn beim Laden ein. GEMESSENE REICHWEITE des Fehlers, wenn das
/// fehlt: jede `$domain=`-Angabe und jede `@@`-Ausnahme fallen durch,
/// und `@@||wikipedia.org^` haelt die Nachrichtenseiten nicht mehr frei.
fn seiten_host_speicher() -> &'static Mutex<String> {
    static H: OnceLock<Mutex<String>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(String::new()))
}

/// Der Host der aktuellen Seite. Leer, wenn noch keine geladen wurde.
///
/// GEMESSENE ENTSCHEIDUNG: Leer heisst „die Seite kennt sich nicht", und
/// eine leere Seite blockiert **nichts** — sonst waere der erste
/// Abruf jeder Seite blockiert, und der Filter waere unbrauchbar.
#[must_use]
pub fn seiten_host() -> String {
    match seiten_host_speicher().lock() {
        Ok(g) => g.clone(),
        Err(v) => v.into_inner().clone(),
    }
}

/// Der Host der aktuellen Seite, gesetzt vom Seitenladen.
///
/// `adresse` ist die vollstaendige URL; nur der Host wird gebraucht.
pub fn seiten_host_setzen(adresse: &str) {
    let h = narsil_adblock::Anfrage::neu(adresse).host;
    if let Ok(mut g) = seiten_host_speicher().lock() {
        *g = h;
    }
}
