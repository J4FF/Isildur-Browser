//! A-ADBLOCK: EasyList-Parser und -Pruefer.
//!
//! # WAS DIESE CRATE KANN
//!
//! * eine EasyList-artige Liste lesen (Netzregeln, Ausnahmen,
//!   kosmetische Regeln),
//! * eine Anfrage dagegen entscheiden — schnell, weil die Regeln mit
//!   `||host^` in einem Hash nach Host und Token liegen,
//! * sagen, welche Selektoren auf einer Seite zu verstecken sind.
//!
//! # WAS SIE NICHT KANN, und warum das eine Entscheidung ist
//!
//! * **Kein URL-Parsen.** Der Aufrufer zerlegt die URL und gibt Host
//!   und Rest. URL-Parsen gehoert `narsil-url`; eine zweite
//!   Implementierung waere eine zweite Wahrheit.
//! * **Kein Netzwerk.** Blockiert wird nicht, sondern entschieden. Der
//!   Aufrufer — `narsil-page` vor `fetch` — macht den Rest.
//! * **Keine Aktualisierung im Lauf.** Die Liste wird einmal geladen.
//!   Eine Hot-Reload-Schleife waere ein Thread (M6) ohne Obergrenze.
//!
//! # GRENZEN (M1–M9)
//!
//! * Alle Schleifen haben eine benannte Obergrenze: [`MAX_REGELN`],
//!   [`MAX_ZEILE`], [`MAX_OPTIONEN`], [`MAX_ANTEIL`], [`MAX_SELEKTOR`],
//!   [`MAX_DOMAIN_ANTEILE`], [`MAX_LABELS`].
//! * Keine Allokation ohne Deckel.
//! * Keine Rekursion (M5).
//! * Kein `unsafe`, kein `unwrap` auf Fremddaten.
//!
//! # BEISPIEL
//!
//! ```no_run
//! use narsil_adblock::{Anfrage, Pruefer, Urteil};
//! use narsil_adblock::typen::AnfrageArt;
//!
//! let p = Pruefer::aus_text(narsil_adblock::grundliste());
//! let a = Anfrage::neu("https://ads.doubleclick.net/pixel.gif")
//!     .mit_seite("wikipedia.org")
//!     .mit_art(AnfrageArt { bild: true, ..AnfrageArt::default() });
//! // Der Index ist die Stellung in der Liste; der Test prueft ihn, der
//! // Aufrufer sollte es nicht.
//! assert!(matches!(p.pruefe(&a), Urteil::Blockiert(_)));
//!
//! // Und eine Ausnahme gewinnt:
//! let frei = Anfrage::neu("https://de.wikipedia.org/wiki/Liste");
//! assert_eq!(p.pruefe(&frei), Urteil::Frei);
//! ```

#![forbid(unsafe_code)]

pub mod parser;
pub mod pruefer;
pub mod regel;
/// Release 08.10.2026: Cookie-Banner- und Stoerer-Listen (Features 9 und 11).
pub mod ruhe;

/// Die Oeffentlichen Formen in einem Modul, damit `use narsil_adblock::…`
/// kurz bleibt. Wer sie braucht, schreibt `narsil_adblock::typen::…`.
pub mod typen {
    pub use crate::regel::{
        Andocken, AnfrageArt, Kosmetisch, MAX_ANTEIL, MAX_DOMAIN_ANTEILE, MAX_OPTIONEN, MAX_REGELN,
        MAX_SELEKTOR, MAX_ZEILE, Netzregel,
    };
}

pub use parser::{Auswertung, Regel, liste_lesen};
/// Die Regeln, die man beim Aufrufen braucht, bekommen einen kurzen Weg.
pub use pruefer::{Anfrage, Pruefer, Urteil};

/// Die eingebaute Grundliste.
///
/// GEMACHT mit `include_str!`, damit sie im Binary liegt: ein Filter,
/// der eine Datei von der Platte lesen muss, braucht einen Pfad, und
/// ein Pfad im Browser hat keine. Wer sie ersetzen will, ruft
/// [`Pruefer::aus_text`] mit einer anderen Liste.
///
/// Herkunft und Lizenz: siehe `daten/grundliste.txt`. Kurzfassung:
/// von Hand fuer dieses Projekt geschrieben, KEIN EasyList-Auszug, MIT
/// wie das Projekt.
pub const GRUNDLISTE: &str = include_str!("../daten/grundliste.txt");

/// Die Grundliste als bereits geparster Pruefer.
///
/// GEMESSENE ENTSCHEIDUNG: das Parsen passiert EINMAL beim Start, nicht
/// je Anfrage. Wer es je Anfrage täte, wuerde bei 50 000 Regeln und
/// 200 Anfragen 10 Millionen Zeilen Arbeit machen.
pub fn grundliste() -> &'static str {
    GRUNDLISTE
}

/// Zahlen der eingebauten Liste, fuer den Bericht und fuer Tests.
///
/// M7: die Zahlen werden ZURUECKGEGEBEN, nicht nur behauptet. Ohne
/// diese Funktion waere „wir haben 40 Regeln" eine Behauptung.
#[must_use]
pub fn grundliste_zahlen() -> GrundlisteZahlen {
    let a = liste_lesen(GRUNDLISTE);
    let p = Pruefer::aus(&a);
    let mut netz = 0usize;
    let mut ausnahmen = 0usize;
    let mut kosmetisch = 0usize;
    for r in a.regeln.iter() {
        match r {
            Regel::Netz(n) if n.ausnahme => ausnahmen += 1,
            Regel::Netz(_) => netz += 1,
            Regel::Kosmetisch(_) => kosmetisch += 1,
        }
    }
    GrundlisteZahlen {
        zeilen: GRUNDLISTE.lines().count(),
        regeln_gesamt: a.regeln.len(),
        netz,
        ausnahmen,
        kosmetisch,
        kommentare: a.kommentare,
        kopfzeilen: a.kopfzeilen,
        leerzeilen: a.leerzeilen,
        unerkannt: a.unerkannt,
        gekuerzt: a.gekuerzt,
        host_im_index: p.host_anzahl(),
        ohne_anker: p.ohne_anker_anzahl(),
    }
}

/// Die Zahlen der eingebauten Liste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrundlisteZahlen {
    /// Zeilen in der Datei, mit Kommentaren.
    pub zeilen: usize,
    /// Angenommene Regeln.
    pub regeln_gesamt: usize,
    /// Netzregeln, die blockieren.
    pub netz: usize,
    /// Netzregeln als Ausnahmen (`@@`).
    pub ausnahmen: usize,
    /// Kosmetische Regeln.
    pub kosmetisch: usize,
    /// Kommentarzeilen.
    pub kommentare: usize,
    /// Listenkoepfe.
    pub kopfzeilen: usize,
    /// Leere Zeilen.
    pub leerzeilen: usize,
    /// Zeilen ohne Regel.
    pub unerkannt: usize,
    /// Zu lange Zeilen.
    pub gekuerzt: usize,
    /// Verschiedene Hosts im Index.
    pub host_im_index: usize,
    /// Regeln ohne Hostanker, also im vollen Durchlauf.
    pub ohne_anker: usize,
}
