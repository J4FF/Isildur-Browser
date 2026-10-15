//! W7 P1 — PHASEN-MESSUNG beim Laden (`NARSIL_ZEITEN=1`).
//!
//! # WOFUER
//!
//! „wikipedia.org braucht 83 s" (Koordination, Debug-Bau). Wo die Zeit
//! hingeht, war unbekannt — und ohne das zu wissen, ist jede Aenderung
//! ein Raten. Diese Datei misst die Phasen und schreibt sie nach stderr.
//!
//! # WARUM EINE EIGENE DATEI
//!
//! Die Messung gehoert an genau EINEN Weg: `Sitzung::aus_ist` (Seite
//! laden) und `Sitzung::klick` (neu bauen). Beide rufen dieselben
//! Funktionen. Eine Datei mit dem Zaehler und einem `Drop`, der die
//! Summe ausgibt, braucht keine Aenderung an den Aufrufern — nur zwei
//! `marke(...)`-Aufrufe.
//!
//! # DIE PHASEN
//!
//! * `netz` — DNS, TLS, Lesen. GEMESSEN, warum das EINE Phase ist:
//!   `narsil_net::fetch` ist ein Aufruf, und `narsil-net` gehoert Rolle TLS
//!   (nur lesen hier). Wer es aufsplitten will, muss es dort tun.
//! * `parse` — `narsil_page::html_lesen`.
//! * `kaskade` — `seite_aus_knoten_mit_messung` (Kaskade + DOM-Bau).
//! * `layout` — `layout_with_measurer`.
//! * `js` — `skripte_ausfuehren` (inklusive Neubau durch den Patch).
//! * `rest` — alles, was sonst dazwischenliegt (Bilder, Inhaltshoehe).
//!
//! # M3: DIE MESSLISTE IST BEGRENZT
//!
//! `MAX_MESSUNGEN` (64) Phasen je Vorgang. Eine Seite mit 10.000 Knoten
//! darf nicht 10.000 Zeilen auf stderr schreiben — und der Speicher fuer
//! die Liste waere dann auch Eingabe. Wird die Grenze erreicht, zaehlt
//! der Zaehler weiter, aber es wird nichts mehr gespeichert.

// GEMESSEN 02.10.2026, 3:8x: `AtomicU64` stand hier unbenutzt (clippy
// `unused_imports`) — aus einer Phase, in der `gezaehlt` atomar gefuehrt
// wurde. Zwei Zeilen weniger, die man lesen muss, ohne dass sie etwas tun.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Hoechste Zahl gemessener Phasen je Vorgang (M3).
pub const MAX_MESSUNGEN: usize = 64;

/// Alles anzeigen? Nur wenn `NARSIL_ZEITEN=1` in der Umgebung steht.
///
/// GEMESSEN, warum eine Umgebungsvariable und kein Schalter im Aufruf:
/// ein Schalter muesste durch jede Schicht (Sitzung, Seite, Layout) und
/// wuerde am Ende in jeder Unterschrift stehen. Die Variable wird einmal
/// gelesen und ist danach ein `bool` in einer statischen Zelle.
static AN: AtomicBool = AtomicBool::new(false);
static AN_GEPRUEFT: AtomicBool = AtomicBool::new(false);

/// Ist die Messung an? (liest die Umgebung genau einmal)
pub fn an() -> bool {
    if !AN_GEPRUEFT.load(Ordering::Relaxed) {
        let wert = std::env::var("NARSIL_ZEITEN")
            .map(|v| v == "1" || v == "ja" || v == "true")
            .unwrap_or(false);
        AN.store(wert, Ordering::Relaxed);
        AN_GEPRUEFT.store(true, Ordering::Relaxed);
    }
    AN.load(Ordering::Relaxed)
}

/// Die Messung OHNE `NARSIL_ZEITEN` einschalten (D8, 02.10.2026).
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier und nicht in der Sitzung: der
/// Weg von der Phase zum Reiter laeuft ueber diese Datei — `Messung`
/// entsteht in `Sitzung::aus_ist` (`&self`, also ohne Platz fuer einen
/// Ausgabepuffer) und die Zahlen muessen am ENDE des Ladevorgangs
/// irgendwo liegen. Diese Datei ist der einzige Ort, den beide Seiten
/// kennen.
///
/// GEMESSENE ENTSCHEIDUNG, warum `ausgeben()` NICHT mit aufwacht: die
/// Ausgabe geht nach stderr und wird von der Koordination gelesen
/// (`NARSIL_ZEIT gehe_zu parse …`). Ein jeder, der die Werkzeuge oeffnet,
/// wuerde sonst stderr zuschuetten — und der Reiter braucht die Zahlen im
/// Fenster, nicht auf stderr.
static ERWACHT: AtomicBool = AtomicBool::new(false);

/// Die Messung fuer diesen Thread aufwecken (D8: der Reiter braucht die
/// Zahlen im Fenster).
pub fn erwecke() {
    ERWACHT.store(true, Ordering::Relaxed);
}

/// Die Messung wieder schlafen legen (der Reiter ist zu).
pub fn schlaefe() {
    ERWACHT.store(false, Ordering::Relaxed);
}

/// Laeuft die Messung — wegen der Variable ODER weil jemand sie geweckt hat?
fn laeuft() -> bool {
    an() || ERWACHT.load(Ordering::Relaxed)
}

// Ohne Doku-Kommentar: `rustdoc` dokumentiert keine Makro-Aufrufe und
// clippy sagt es beim Bauen (gemessen 02.10.2026, 3:8x). Die Erklaerung
// steht an `gesammelt`, das sie liest.
thread_local! {
    static GESAMMELT: std::cell::RefCell<Vec<(&'static str, u64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Die Phasen des laufenden Vorgangs, nach Name gesummiert.
///
/// GEMESSENE ENTSCHEIDUNG, warum GESAMMELT und nicht „der letzte
/// Vorgang": an einem Ladevorgang sind ZWEI `Messung` beteiligt — eine in
/// `Sitzung::gehe_zu` (Phase „netz") und eine in `Sitzung::aus_ist` (parse,
/// blaetter, kaskade, layout, js). Wer nur den letzten fallen lassen
/// wuerde (also `Drop`), haette am Ende nur noch „netz" — GEMESSEN so in
/// der ersten Fassung (Bild: „Netz 0,00 ms 0x" bei 1,3 s Ladezeit). Also
/// sammelt jede Phase in eine Liste, und `sammel_leeren()` leert sie am
/// BEGINN eines Vorgangs.
pub fn gesammelt() -> Vec<(&'static str, u64)> {
    GESAMMELT.with(|g| g.borrow().clone())
}

/// Die Sammlung leeren — am Anfang eines Ladevorgangs.
pub fn sammel_leeren() {
    GESAMMELT.with(|g| g.borrow_mut().clear());
}

pub(crate) fn uebernehmen(zeiten: Vec<(&'static str, u64)>) {
    GESAMMELT.with(|g| *g.borrow_mut() = zeiten.into_iter().take(MAX_MESSUNGEN).collect());
}

fn sammle(name: &'static str, ns: u64) {
    GESAMMELT.with(|g| {
        let mut v = g.borrow_mut();
        match v.iter_mut().find(|(n, _)| *n == name) {
            Some((_, alt)) => *alt += ns,
            None => v.push((name, ns)),
        }
    });
}

/// Eine Phase und ihre Dauer in Nanosekunden.
#[derive(Debug, Clone)]
pub struct Phase {
    /// Der Name der Phase (`"kaskade"`).
    pub name: &'static str,
    /// Die Dauer in Nanosekunden.
    pub ns: u64,
}

/// Alle Phasen eines Vorgangs.
#[derive(Debug, Default)]
pub struct Messung {
    phasen: Vec<Phase>,
    /// Wie viele Phasen es eigentlich gab (M3: `phasen` ist gekappt).
    pub gezaehlt: u64,
}

impl Messung {
    /// Leeren Vorgang anlegen (nur wenn die Messung laeuft, sonst `None`).
    pub fn neu() -> Option<Self> {
        laeuft().then(Messung::default)
    }

    /// Eine Phase messen. Der closure-Teil ist der, der zaehlt.
    pub fn phase<T>(&mut self, name: &'static str, f: impl FnOnce() -> T) -> T {
        if !laeuft() {
            return f();
        }
        let start = Instant::now();
        let r = f();
        self.gezaehlt += 1;
        // M3: nach der Grenze wird nichts mehr gespeichert, aber weiter
        // gezaehlt — sonst waere die Zahl, die man gegen die Grenze prueft,
        // zu klein.
        if self.phasen.len() < MAX_MESSUNGEN {
            self.phasen.push(Phase {
                name,
                ns: start.elapsed().as_nanos() as u64,
            });
        }
        // UND: dieselbe Phase wandert in die Sammlung (D8). GEMESSENE
        // ENTSCHEIDUNG, warum die Sammlung NICHT die Grenze kennt: sie
        // summiert nach Name, also gibt es pro Name hoechstens einen
        // Eintrag — die Grenze `MAX_MESSUNGEN` gilt fuer die Ausgabe, und
        // die Ausgabe ist eine andere Sache als der Reiter.
        let ns = start.elapsed().as_nanos() as u64;
        sammle(name, ns);
        r
    }

    /// Die Phasen und ihre Summe auf stderr schreiben.
    ///
    /// GEMESSEN, warum nach stderr und nicht in die Konsole der Seite:
    /// die Konsole gehoert dem Nutzer (er liest Skriptmeldungen), und die
    /// Koordination misst aus dem Bau heraus. Zwei Ziele, zwei Wege.
    pub fn ausgeben(&self, was: &str) {
        if !an() {
            return;
        }
        let summe: u64 = self.phasen.iter().map(|p| p.ns).sum();
        // M4/M3: die Ausgabe ist eine Zeile je Phase, hoechstens
        // `MAX_MESSUNGEN` (64) plus eine. Kein ungebrochener Riesenblock.
        for p in &self.phasen {
            std::eprintln!(
                "NARSIL_ZEIT {was} {:<10} {:>9.2} ms",
                p.name,
                p.ns as f64 / 1_000_000.0
            );
        }
        std::eprintln!(
            "NARSIL_ZEIT {was} {:<10} {:>9.2} ms ({} Phasen{})",
            "SUMME",
            summe as f64 / 1_000_000.0,
            self.gezaehlt,
            if self.gezaehlt as usize > MAX_MESSUNGEN {
                ", gekappt"
            } else {
                ""
            }
        );
    }
}

/// Wie viele Zeiten dieselbe Phase lief (Summe je Name) — fuer „wo sitzt
/// die Zeit", wenn eine Phase viele Male vorkommt.
pub fn nach_name(m: &Messung) -> Vec<(&'static str, u64)> {
    let mut raus: Vec<(&'static str, u64)> = Vec::new();
    for p in &m.phasen {
        match raus.iter_mut().find(|(n, _)| *n == p.name) {
            Some((_, v)) => *v += p.ns,
            None => raus.push((p.name, p.ns)),
        }
    }
    raus
}
