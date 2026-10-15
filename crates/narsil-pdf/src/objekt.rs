//! Das Objektmodell von PDF — die Werte, die ein Dokument enthalten kann.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF.
//!
//! # WARUM EIGENE TYPEN UND KEIN `serde_json::Value`
//!
//! PDF hat zwei Dinge, die JSON nicht kann, und beide sind der Grund
//! fuer diesen Typ: **Referenzen** (`7 0 R`) muessen erhalten bleiben,
//! weil ein Objekt auf ein anderes zeigt und nicht kopiert werden darf,
//! und **Streams** tragen Daten, die nicht im Wörterbuch stehen.
//! Wer beides in einen Baum kopiert, verliert die Beziehungen.
//!
//! # DIE ENTSCHEIDUNG, die alles weitere bestimmt
//!
//! Ein Wörterbuch ist `Vec<(Vec<u8>, Wert)>` und **keine** `HashMap`.
//! Grund: doppelte Schlüssel sind in PDF erlaubt (der spätere gewinnt
//! laut Spezifikation), und eine `HashMap` würde die Reihenfolge
//! zerstören — und mit ihr die **Reihenfolge der Content-Stream-
//! Operatoren** in Resources. Das ist keine Feinheit: ein Dokument,
//! bei dem die Zeichenreihenfolge stimmt und die Füllreihenfolge nicht,
//! sieht falsch aus und laesst sich schwer finden.

/// Ein Wert aus einem PDF-Dokument.
///
/// added 03.10.2026.
#[derive(Debug, Clone, PartialEq)]
pub enum Wert {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// Eine Zahl. PDF kennt nur eine Zahlenart; Integer werden auch
    /// hier abgelegt, wenn sie keine Nachkommastelle haben (siehe
    /// [`Wert::als_int`]).
    Zahl(f64),
    /// Ein Name: `/Type`. Die Bytes sind **ohne** den Schraegstrich und
    /// **mit** aufgeloesten `#xx`-Fluchtzeichen.
    Name(Vec<u8>),
    /// Ein Klartextstring in Klammern. Die Bytes sind der **Inhalt**,
    /// ohne Klammern, mit aufgeloesten Fluchtzeichen.
    String(Vec<u8>),
    /// `[ ... ]`
    Array(Vec<Wert>),
    /// `<< ... >>`
    Dict(Vec<(Vec<u8>, Wert)>),
    /// `7 0 R` — ein Verweis auf Objekt 7, Generation 0.
    ///
    /// Die Generation steht hier, wird aber von keinem modernen
    /// Dokument benutzt. Sie wird mitgefuehrt, weil ein Verweis ohne
    /// sie nicht unterscheidbar waere von einem `7 0` in einem Array.
    Ref(u32, u16),
    /// Ein Stream: Wörterbuch plus Rohdaten.
    ///
    /// Die Daten sind **nicht** dekodiert. Erst wer `/Filter` liest,
    /// weiss, was darin steckt — und ein Dokument, das einen unbekannten
    /// Filter benutzt, ist lesbar, nur der Inhalt nicht.
    Stream(Vec<(Vec<u8>, Wert)>, Vec<u8>),
    /// Ein Content-Stream-Operator (`re`, `f`, `Tj`, `BT`, ...).
    ///
    /// Steht in `Wert`, weil ein Content-Stream technisch eine Folge
    /// von Operanden und Operatoren ist und beides durch denselben
    /// Lexer laeuft.
    Op(String),
    /// Das Ende einer Content-Stream-Operandenfolge.
    ///
    /// needed, weil `TJ` ein Array von Zahlen **und** Strings
    /// erwartet, und ein Array aus lauter Strings sonst kein Ende hat.
    EndeOperanden,
}

impl Wert {
    /// Der Name als Text, wenn es einer ist.
    ///
    /// added 03.10.2026. Kein `String::from_utf8_lossy` im Aufrufer —
    /// die Frage „ist es ein Name mit diesem Text?" wird an **einer**
    /// Stelle gestellt.
    pub fn name(&self) -> Option<&str> {
        match self {
            Wert::Name(b) => std::str::from_utf8(b).ok(),
            _ => None,
        }
    }

    /// Der Name als Bytes (Vergleich ohne UTF-8-Umweg).
    pub fn name_bytes(&self) -> Option<&[u8]> {
        match self {
            Wert::Name(b) => Some(b),
            _ => None,
        }
    }

    /// Die Zahl, wenn es eine ist.
    pub fn zahl(&self) -> Option<f64> {
        match self {
            Wert::Zahl(z) => Some(*z),
            _ => None,
        }
    }

    /// Die Zahl als Ganzzahl, wenn sie ganzzahlig ist.
    ///
    /// added 03.10.2026. `1.0` ist eine Zahl und **kein** Ganzzahl, wenn
    /// man streng ist; PDF-Generatoren schreiben aber `0` als `0.0` in
    /// manchen Faellen. Deshalb: ganzzahlig **und** im `i64`-Bereich.
    pub fn als_int(&self) -> Option<i64> {
        match self {
            Wert::Zahl(z) => {
                if z.is_finite() && *z >= -(2f64.powi(63)) && *z < 2f64.powi(63) && z.fract() == 0.0
                {
                    Some(*z as i64)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Das Wörterbuch, wenn der Wert einer ist.
    ///
    /// added 03.10.2026. **Und** der Stream: ein Stream *ist* ein
    /// Wörterbuch mit Daten, und der halbe Formularcode fragt nach
    /// `/Filter` oder `/Type`, ohne den Stream zu unterscheiden.
    pub fn dict(&self) -> Option<&Vec<(Vec<u8>, Wert)>> {
        match self {
            Wert::Dict(d) => Some(d),
            Wert::Stream(d, _) => Some(d),
            _ => None,
        }
    }

    /// Das Array, wenn es eines ist.
    pub fn array(&self) -> Option<&Vec<Wert>> {
        match self {
            Wert::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Die Stream-Daten, wenn es ein Stream ist.
    pub fn stream_daten(&self) -> Option<&[u8]> {
        match self {
            Wert::Stream(_, d) => Some(d),
            _ => None,
        }
    }

    /// Der Wert fuer einen Schluessel, wenn es ein Wörterbuch ist.
    ///
    /// added 03.10.2026. **Der letzte** Eintrag gewinnt, weil das die
    /// Spezifikation sagt und weil ein `HashMap::insert` in einem
    /// Parser still den ersten gewinnen laesst.
    pub fn get(&self, schluessel: &str) -> Option<&Wert> {
        self.dict()?
            .iter()
            .rev()
            .find(|(k, _)| k.as_slice() == schluessel.as_bytes())
            .map(|(_, v)| v)
    }

    /// Wie [`Wert::get`], aber mit Rueckgabe des Werts als Zahl.
    pub fn get_zahl(&self, schluessel: &str) -> Option<f64> {
        self.get(schluessel).and_then(Wert::zahl)
    }

    /// Wie [`Wert::get`], aber mit Rueckgabe als Ganzzahl.
    pub fn get_int(&self, schluessel: &str) -> Option<i64> {
        self.get(schluessel).and_then(Wert::als_int)
    }

    /// Wie [`Wert::get`], aber mit Rueckgabe als Name (Text).
    pub fn get_name(&self, schluessel: &str) -> Option<&str> {
        self.get(schluessel).and_then(Wert::name)
    }

    /// Wie [`Wert::get`], aber mit Rueckgabe als Referenz.
    ///
    /// added 03.10.2026. Weil `/Parent 2 0 R` immer eine Referenz ist
    /// und `/Kids` immer ein Array von Referenzen — an **einer** Stelle
    /// geprueft statt an vier.
    pub fn get_ref(&self, schluessel: &str) -> Option<(u32, u16)> {
        match self.get(schluessel) {
            Some(Wert::Ref(n, g)) => Some((*n, *g)),
            _ => None,
        }
    }

    /// Kuerzt eine Namens-Darstellung fuer Fehlermeldungen.
    ///
    /// added 03.10.2026. Ein Fehlertext mit einem 300-Byte-Namen ist
    /// unlesbar, und unlesbare Fehlertexte werden ignoriert.
    pub fn name_kurz(&self, max: usize) -> String {
        let voll = match self {
            Wert::Name(b) => String::from_utf8_lossy(b).into_owned(),
            other => format!("{other:?}"),
        };
        if voll.chars().count() <= max {
            return voll;
        }
        let kopf: String = voll.chars().take(max.saturating_sub(3)).collect();
        format!("{kopf}...")
    }
}

/// Der Formfehler: eine Objektnummer, die es nicht gibt.
///
/// added 03.10.2026. Eigener Typ, weil „Objekt nicht gefunden" kein
/// Syntaxfehler ist, sondern ein **Laufzeit**befund — und weil ein
/// Dokument mit einem kaputten Verweis immer noch lesbar ist.
#[derive(Debug, Clone, PartialEq)]
pub struct FehlenderVerweis {
    /// Nummer des gesuchten Objekts.
    pub nummer: u32,
    /// Generation des gesuchten Objekts.
    pub generation: u16,
}

impl std::fmt::Display for FehlenderVerweis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Objekt {} {} R fehlt", self.nummer, self.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Das Wörterbuch findet den **letzten** doppelten Schluessel.
    ///
    /// added 03.10.2026. Ohne diesen Test koennte jemand `HashMap`
    /// einbauen und die Regel unabsichtlich drehen.
    #[test]
    fn doppelter_schluessel_letzter_gewinnt() {
        let w = Wert::Dict(vec![
            (b"Type".to_vec(), Wert::Name(b"A".to_vec())),
            (b"Type".to_vec(), Wert::Name(b"B".to_vec())),
        ]);
        assert_eq!(w.get_name("Type"), Some("B"));
    }

    /// `/Kids` als Referenz und als Zahl gehoert unterschieden.
    ///
    /// added 03.10.2026. `/Count 3` ist eine Zahl, `/Parent 2 0 R` eine
    /// Referenz; wer beides verwechselt, laeuft in einen
    /// `E0425`-aehnlichen Fehler an einer Stelle, die man nicht sucht.
    #[test]
    fn referenz_und_zahl_unterscheiden() {
        let w = Wert::Dict(vec![
            (b"Count".to_vec(), Wert::Zahl(3.0)),
            (b"Parent".to_vec(), Wert::Ref(2, 0)),
        ]);
        assert_eq!(w.get_int("Count"), Some(3));
        assert_eq!(w.get_ref("Parent"), Some((2, 0)));
        assert!(w.get_ref("Count").is_none());
    }

    /// `1.0` ist ganzzahlig, `1.5` nicht, `unendlich` nicht.
    ///
    /// added 03.10.2026. Die dritte Zeile ist der Bombenfall: ein
    /// Verweis auf ein Objekt mit unendlicher Seitenzahl darf nicht
    /// durch `als i64` (saturiert) zu einer Zahl werden.
    #[test]
    fn als_int_ist_streng() {
        assert_eq!(Wert::Zahl(1.0).als_int(), Some(1));
        assert_eq!(Wert::Zahl(1.5).als_int(), None);
        assert_eq!(Wert::Zahl(f64::INFINITY).als_int(), None);
        assert_eq!(Wert::Zahl(1e30).als_int(), None);
    }
}
