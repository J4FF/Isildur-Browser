//! KOSMETISCHE REGELN ALS KASKADE — der Weg von `##selektor` zu
//! `display: none`.
//!
//! # WARUM DIES HIER UND NICHT IN `narsil-adblock`
//!
//! `narsil-adblock` liefert eine Liste von Selektoren. Was daraus in der
//! Kaskade wird, ist eine Frage der CSS-Crate: `narsil-adblock` ist
//! absichtlich nur `std` (ADR-030) und soll nicht von `narsil-css`
//! abhaengen — dann koennte man es auch dort einbauen, und die beiden
//! Crates wuerden untrennbar.
//!
//! # DIE EINE ENTSCHEIDUNG: `!important`
//!
//! EasyList versteckt Elemente mit CSS-Regeln, und die muessen eine
//! Seitenregel schlagen. Ohne `!important` verliert jede Regel mit
//! gleicher oder hoherer Spezifitaet gegen die Seite — und Werbebanner
//! werden fast immer mit ID oder Klasse daher, also gewinnen sie.
//!
//! GEMESSENE KONSEQUENZ, und sie ist eine Entscheidung: `display: none
//! !important` schlaegt auch eine `!important`-Regel der Seite. Das ist
//! Absicht (das ist der Zweck eines Werbefilters), aber es heisst auch:
//! wer diese Liste benutzt, nimmt der Seite die Moeglichkeit, ein
//! Werbeelement sichtbar zu erzwingen. Fuer einen Browser mit
//! eingebautem Filter ist das die richtige Entscheidung; fuer eine
//! Werkzeugleiste waere sie es nicht.
//!
//! # EINE REGEL PRO SELZEKTOR, nicht eine fuer alle
//!
//! GEMESSENE BEGRUENDUNG: ein kaputter Selektor darf nicht die ganze
//! Liste mitnehmen. Bei einer gemeinsamen Regel
//! `.a,.kaputt,.b { … }` faellt die ganze Regel aus. Bei einer Regel je
//! Selektor faellt nur der eine aus.

use crate::MAX_REGELN;
use crate::typen::Stylesheet;

/// Die Anweisung, mit der ein Element versteckt wird.
pub const VERSTECKEN: &str = "display: none !important";

/// Wandelt Selektoren in ein Blatt fuer die Kaskade.
///
/// # GRENZEN (M3/M4)
///
/// * Hoechstens [`MAX_REGELN`] Selektoren. Der Wert ist derselbe wie
///   fuer jede Regelmenge in dieser Crate — eine kosmetische Liste,
///   die groesser ist, ist keine Feinheit mehr, sondern ein Kaskaden-
///   Sprung in der Ladezeit.
/// * [`MAX_SELEKTOREN`] je Selektor-Text: ein 10-MB-Selektor wird
///   gekuerzt und der Kuerzungsfall gezaehlt, damit man ihn sieht.
///
/// # M7
///
/// `KosmetikErgebnis` gibt die Zahlen ZURUECK: wie viele Regeln
/// entstanden sind, wie viele an der Grenze gekuerzt und wie viele der
/// CSS-Text selbst zerlegt hat. Ohne diese Zahlen waere „eingebaut"
/// eine Behauptung.
// `Eq` gibt es bei `Stylesheet` nicht, darum nur `PartialEq`. Ein
// Ergebnis mit Blatt vergleicht man ueber die ZAHLEN, nicht ueber den
// Baum — der enthaelt Floats.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KosmetikErgebnis {
    /// Das Blatt, das in die Liste der Blaetter gehoert.
    pub blatt: Stylesheet,
    /// Angebotene Selektoren.
    pub angeboten: usize,
    /// Daraus gewordenen Regeln.
    pub regeln: usize,
    /// Selektoren, die an [`MAX_SELEKTOREN`] gekuerzt wurden.
    pub gekuerzt: usize,
    /// Selektoren, die ab [`MAX_REGELN`] weggelassen wurden.
    pub weggelassen: usize,
    /// Selektoren, die gar nicht erst gebaut wurden, weil sie ein
    /// `;`, `{` oder `}` enthalten — ein `;` wuerde die Regel
    /// zerlegen, die Klammern wuerden den CSS-Text abschneiden.
    ///
    /// GEMESSENE ENTSCHEIDUNG: getrennt von `unlesbar`, weil die
    /// Ursachen verschieden sind. `unlesbar` heisst „der CSS-Parser
    /// kannte `:has-text()` nicht", `verworfen` heisst „wir haben es
    /// ihm erst gar nicht gegeben". Beide Zahlen zu einer zu machen
    /// wuerde zwei verschiedene Luecken eine sein lassen.
    pub verworfen: usize,
    /// Selektoren, die der CSS-Text nicht verstanden hat.
    /// GEMESSENE BEDEUTUNG: Ein Adblock-Selektor darf eine Erweiterung
    /// der Seite benutzen (`:has-text()`, `-ext-`), die es in CSS nicht
    /// gibt. Solche Selektoren landen hier, und ihre Zahl ist die
    /// Groesse dieser Luecke.
    pub unlesbar: usize,
}

/// Hoechste Laenge eines Selektor-Texts (M4).
///
/// 512 Byte sind laenger als jeder reale Selektor.
pub const MAX_SELEKTOREN: usize = 512;

/// Baut das Blatt.
#[must_use]
pub fn kosmetik_als_blatt(selektoren: &[&str]) -> KosmetikErgebnis {
    let (text, gekuerzt, weggelassen, verworfen) = baue(selektoren);
    KosmetikErgebnis::aus_text(&text, selektoren.len(), gekuerzt, weggelassen, verworfen)
}

impl KosmetikErgebnis {
    /// GEMESSENER FEHLER, den diese Zeile verursacht hat: Ich habe hier
    /// `gekuerzt: 0`, `weggelassen: 0` und `unlesbar: 0` fest verdrahtet
    /// und in `kosmetik_als_css` zwar `gekuerzt` GEZAEHLT, aber nie
    /// herausgegeben. Das Ergebnis war damit eine Zahl, die immer 0
    /// war — und M7 sagt, jede Zahl wird gesichert zurueckgegeben.
    /// Mein Test hat es nicht gefunden, weil er nur den Normalfall
    /// pruefte (`gekuerzt == 0` bei drei kurzen Selektoren).
    ///
    /// Jetzt kommen die Zahlen aus `baue()` und nicht aus dem Nichts.
    fn aus_text(
        text: &str,
        angeboten: usize,
        gekuerzt: usize,
        weggelassen: usize,
        verworfen: usize,
    ) -> KosmetikErgebnis {
        let blatt = crate::stylesheet_lesen(text);
        let regeln = blatt.regeln.len();
        KosmetikErgebnis {
            blatt,
            angeboten,
            regeln,
            gekuerzt,
            weggelassen,
            verworfen,
            // Alles, was weder Kuerzung noch Verworfen noch Regel ist:
            // der CSS-Parser hat es nicht verstanden. Bei
            // `div:has-text(werbung)` ist das der Fall.
            unlesbar: angeboten
                .saturating_sub(gekuerzt)
                .saturating_sub(verworfen)
                .saturating_sub(regeln),
        }
    }
}

/// Der CSS-Text fuer eine Selektorliste.
///
/// # LEERZEILEN UND KAPSELN
///
/// `Entweder 0 oder 1 (Anfuehrungszeichen)` werden entfernt: ein leerer
/// Selektor ergaebe ` { … }` und wuerde den Parser zum Abbruch
/// bringen; ein `;` darin wuerde die Regel zerlegen. Beides sind
/// kaputte Angaben, und beide duerfen die Liste nicht anhalten.
pub fn kosmetik_als_css(selektoren: &[&str]) -> String {
    baue(selektoren).0
}

/// Der CSS-Text UND die Zahlen, in EINEM Durchlauf.
///
/// GEMESSENE BEGRUENDUNG: Vorher hat `kosmetik_als_css` die Kuerzungen
/// gezaehlt und die Zahl weggeworfen; `anzahl_gekuerzt()` lief ein
/// zweites Mal durch die Liste. Zwei Durchlaeufe fuer eine Zahl, und
/// die Zahl war trotzdem falsch im Ergebnis. Jetzt entstehen Text und
/// Zahlen zusammen — ein Durchlauf, und die Zahl im Ergebnis ist
/// dieselbe, mit der gebaut wurde.
fn baue(selektoren: &[&str]) -> (String, usize, usize, usize) {
    let mut s = String::new();
    let mut gekuerzt = 0usize;
    let mut verworfen = 0usize;
    let mut weggelassen = 0usize;
    for (i, roh) in selektoren.iter().enumerate() {
        if i >= MAX_REGELN {
            // GEMESSENER FEHLER, zweiter in derselben Datei: Das `break`
            // hat alle restlichen Selektoren stillschweigend fallen
            // lassen, und `weggelassen` stand fest auf 0. M7 sagt, jede
            // Zahl wird zurueckgegeben. Bei 5000 kosmetischen Regeln
            // (das ist die Groessenordnung einer grossen Filterliste)
            // waeren das Tausende lautlose Verluste.
            weggelassen += selektoren.len() - i;
            break;
        }
        let t = roh.trim().trim_matches('"').trim();
        if t.is_empty() {
            continue;
        }
        let t = if t.len() > MAX_SELEKTOREN {
            gekuerzt += 1;
            &t[..MAX_SELEKTOREN]
        } else {
            t
        };
        // Ein `;` wuerde die Regel zerlegen, ein `{`/`}` die Klammern.
        // Solche Selektoren werden uebersprungen statt eingebaut — und
        // GEZAEHLT, weil eine Liste, die stillschweigend schrumpft, nicht
        // mehr weiss, wie gross sie geworden ist.
        if t.contains(';') || t.contains('{') || t.contains('}') {
            verworfen += 1;
            continue;
        }
        s.push_str(t);
        s.push('{');
        s.push_str(VERSTECKEN);
        s.push('}');
    }
    (s, gekuerzt, weggelassen, verworfen)
}

/// Wie viele Selektoren wurden gekuerzt? Ohne zweiten Durchlauf geht
/// das nicht — deshalb wird es hier nachgezaehlt.
///
/// GEMESSENE ENTSCHEIDUNG: lieber nachzaehlen als eine
/// `Rc<Cell<usize>>` durch die Funktion ziehen. Die Liste ist einmal
/// pro Seite, nicht je Selektor.
#[must_use]
pub fn anzahl_gekuerzt(selektoren: &[&str]) -> usize {
    baue(selektoren).1
}

/// M7: auch die Zahl der WEGGELASSENEN wird zurueckgegeben.
#[must_use]
pub fn anzahl_weggelassen(selektoren: &[&str]) -> usize {
    baue(selektoren).2
}
