//! G8b: EIN FEHLER FUER DEN GANZEN DEKODER.
//!
//! # WARUM NICHT PRO MODUL EIN FEHLER
//!
//! Der Vorbis-Dekoder hat fuenf Stellen, an denen ein Bitstrom falsch
//! gelesen werden kann: Codebuch, Floor, Residue, Zuordnung, Paket.
//! Jede Stelle hat ihre eigene Fehlerliste. Wer sie einzeln fuehrt,
//! braucht an jeder Uebergabestelle eine Umwandlung — und an der
//! Uebergabestelle steht dann meist ein `map_err(|_| "irgendwas")`, das
//! die Zahl wegwirft, die man zum Nachmessen braucht.
//!
//! DESHALB: ein Fehler mit einer **Art** (was ist passiert) und einem
//! **Text** (mit den Zahlen, die man braucht, um es zu reproduzieren).
//! Die Zahl steht im Text, und ein Test kann `art` vergleichen und den
//! Text zitieren.
//!
//! GEMESSENER GRUND, warum die Zahl im Text und nicht in Feldern: die
//! Fehler, die in dieser Crate wirklich aufgetreten sind, waren alle
//! Zwei-Zahlen-Fehler — „Kopf bei Byte 23", „erwartet 5, gelesen 140",
//! „brauchte 64 Bit, da sind 33736". Eine Art ohne Zahl beantwortet die
//! Frage „an welcher Stelle" nicht, und die ist die erste Frage.

use crate::bitleser::BitFehler;

/// WAS passiert ist — die kleine, vergleichbare Liste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    /// Der Strom endete, bevor genug Bit da waren.
    StromZuEnde,
    /// Eine Zahl aus dem Strom passte nicht in den Zieltyp.
    ZahlZuGross,
    /// Ein Sync-Muster (die 24 Bit `0x564342`) war nicht da.
    FalschesMuster,
    /// Ein Zahlenwert, den die Spezifikation nicht zulaesst.
    FalscherWert,
    /// Zwei Laengen, die nicht zusammenpassen.
    FalscheGroesse,
    /// Dieser Zweig ist bewusst nicht gebaut. Steht IMMER mit einem
    /// Grund im Text — ein `NichtImplementiert` ohne Grund ist eine
    /// Ausrede, keine Luecke.
    NichtImplementiert,
}

impl Art {
    /// Der Kurzname, fuer Testausgaben.
    #[must_use]
    pub fn kurz(&self) -> &'static str {
        match self {
            Art::StromZuEnde => "StromZuEnde",
            Art::ZahlZuGross => "ZahlZuGross",
            Art::FalschesMuster => "FalschesMuster",
            Art::FalscherWert => "FalscherWert",
            Art::FalscheGroesse => "FalscheGroesse",
            Art::NichtImplementiert => "NichtImplementiert",
        }
    }
}

/// Der Fehler: eine Art und ein Text mit den Zahlen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fehler {
    /// Was passiert ist.
    pub art: Art,
    /// Wo und mit welchen Zahlen.
    pub text: String,
}

impl Fehler {
    /// Der Kurzname plus der Text — eine Zeile, die man zitieren kann.
    pub fn neu(art: Art, text: impl Into<String>) -> Self {
        Fehler {
            art,
            text: text.into(),
        }
    }

    /// `NichtImplementiert` **mit Grund**. Ohne Grund wird das nie
    /// benutzt: die Crate-Regel sagt, unfertiges als benannte Luecke zu
    /// melden, und ein Grund ist das, was die Luecke von einer Ausrede
    /// unterscheidet.
    pub fn nicht_gemacht(grund: impl Into<String>) -> Self {
        Fehler::neu(Art::NichtImplementiert, grund)
    }
}

impl std::fmt::Display for Fehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.art.kurz(), self.text)
    }
}

impl std::error::Error for Fehler {}

impl From<BitFehler> for Fehler {
    fn from(f: BitFehler) -> Self {
        // GEMESSENE ENTSCHEIDUNG, warum der BitFehler-Text UEBERNOMMEN
        // und nicht neu gebaut wird: er hat schon beide Zahlen
        // (`brauchte`, `hatte`), und der Text steht an der Stelle, an der
        // danach gesucht wird — naemlich im Protokoll des Absturzes.
        let art = match f {
            BitFehler::ZuEnde { .. } => Art::StromZuEnde,
            BitFehler::VorlaeufigZuEnde { .. } => Art::ZahlZuGross,
            BitFehler::ZuGross { .. } => Art::ZahlZuGross,
        };
        Fehler::neu(art, f.to_string())
    }
}

/// Das Ergebnis der eigenen Schritte.
pub type Ergebnis<T> = Result<T, Fehler>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_fehler_nennt_seine_art_und_seine_zahlen() {
        let f = Fehler::neu(Art::FalscherWert, "Pakettyp 140, erwartet 5");
        assert_eq!(f.art, Art::FalscherWert);
        assert_eq!(f.to_string(), "[FalscherWert] Pakettyp 140, erwartet 5");
    }

    #[test]
    fn der_stromfehler_behaelt_beide_zahlen() {
        // GEMESSEN: die Zahlen sind das Einzige, was beim Debuggen
        // weiterhilft. Deshalb wandert der Text aus `BitFehler` mit.
        let b = BitFehler::ZuEnde {
            brauchte: 64,
            hatte: 33736,
        };
        let f: Fehler = b.into();
        assert_eq!(f.art, Art::StromZuEnde);
        assert!(
            f.text.contains("33736") && f.text.contains("64"),
            "beide Zahlen muessen im Text stehen, steht: {}",
            f.text
        );
    }

    #[test]
    fn eine_luecke_nennt_ihren_grund() {
        // M3: `NichtImplementiert` ohne Grund wird verboten. Der Test
        // haelt die Regel fest, nicht eine Zahl.
        let f = Fehler::nicht_gemacht(
            "Floor 0: die Kurvengleichungen sind im \
             HTML nicht als Text vorhanden und kein Encoder seit Beta 4 \
             schreibt Floor 0",
        );
        assert_eq!(f.art, Art::NichtImplementiert);
        assert!(f.text.len() > 20, "ein Grund, kein Stichwort");
    }

    #[test]
    fn jede_art_hat_einen_kurzen_namen() {
        for a in [
            Art::StromZuEnde,
            Art::ZahlZuGross,
            Art::FalschesMuster,
            Art::FalscherWert,
            Art::FalscheGroesse,
            Art::NichtImplementiert,
        ] {
            assert!(!a.kurz().is_empty());
        }
    }
}
