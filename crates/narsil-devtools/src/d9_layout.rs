//! D19 — LAYOUT/ELEMENT-PANEL: Box-Werte bearbeiten, Regeln mit Herkunft,
//! Eigenschaften an/aus, Barrierefreiheits-Rolle.
//!
//! # WAS DER AUFTRAG VERLANGT (04.10.2026, Leitung)
//!
//! > Reiter „Layout/Element" wie Firefox (Box-Modell mit Wert-Bearbeitung
//! > live, Regeln-Panel: angewendete CSS-Regeln pro Element mit Quelle
//! > Datei:Zeile, Eigenschaften an/aus-Schalter, Wert bearbeiten -> Seite
//! > aktualisiert sofort)
//!
//! # WAS HIER EIGENTLICH IST UND WAS DER AUFRUFER BRINGT
//!
//! GEMESSENE ENTSCHEIDUNG (die gilt fuer die ganze Datei): **das Werkzeug
//! rechnet keine Kaskade.** Es zeigt Werte, merkt sich, was der Nutzer
//! eintippt, und sagt, WAS sich aendern muss. Die Aenderung an der Seite
//! braucht `narsil-page`/`narsil-css` (fremd) — dort steht die
//! `Berechnet`-Struktur, dort wird sie neu berechnet.
//!
//! | Teil | hier | beim Aufrufer |
//! |---|---|---|
//! | die 16 Box-Werte | ja (aus `narsil_css::Berechnet`) | — |
//! | das Editierfeld + der getippte Wert | ja | Taste/Enter weiterreichen |
//! | die **Aenderung** (`margin-left: 12px`) | ja (als Wert) | anwenden + neu rechnen |
//! | die Regeln mit Herkunft | Zeilen aus `narsil_css::StilRegel` | welche passen + Datei/Zeile |
//! | Eigenschaft an/aus | ja (Zustand) | beim Anwenden beruecksichtigen |
//! | Rolle/Name (Barrierefreiheit) | ja (aus Tag + Attributen + Text) | — |
//!
//! # DIE HERKUNFT DER REGELN — GEMESSEN, nicht geraten (04.10.)
//!
//! Der Auftrag nennt „mit Quelle Datei:Zeile". GEMESSEN an
//! `crates/narsil-css/src/typen.rs`: `StilRegel` hat `selektoren`,
//! `deklarationen`, `variablen` — **keine Datei und keine Zeile**. Auch
//! `Stylesheet` (Zeile 21) traegt nur `regeln` und `verworfen`.
//!
//! Also: `RegelZeile::quelle` kommt vom AUFRUFER (der weiss, welches
//! Stylesheet er gelesen hat), und `RegelZeile::zeile` ist `Option<u32>` —
//! `None` heisst „unbekannt", und im Bild steht dann die Datei OHNE Zeile.
//! Erfunden wird nichts. Gemeldet in `team/BRETT.txt` (04.10., an CSS).

use crate::grenzen::Grenzen;
use narsil_css::{Berechnet, Deklaration, Regel, Selektor, Stylesheet};

/// Die vier Ebenen des Box-Modells (die Reihenfolge von aussen nach innen —
/// dieselbe wie `zeichnen::BOX_EBENEN`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxEbene {
    /// `margin`.
    Marge,
    /// `border`.
    Rand,
    /// `padding`.
    Fuellung,
    /// `width`/`height` (der Inhalt).
    Inhalt,
}

impl BoxEbene {
    /// Die vier Ebenen in der Reihenfolge des Bildes.
    pub const ALLE: [BoxEbene; 4] = [
        BoxEbene::Marge,
        BoxEbene::Rand,
        BoxEbene::Fuellung,
        BoxEbene::Inhalt,
    ];

    /// Der Name, wie er im Reiter steht.
    pub const fn name(self) -> &'static str {
        match self {
            BoxEbene::Marge => "Marge",
            BoxEbene::Rand => "Rahmen",
            BoxEbene::Fuellung => "Fuellung",
            BoxEbene::Inhalt => "Inhalt",
        }
    }

    /// Die CSS-Eigenschaft, die der Aufrufer setzen muss.
    ///
    /// GEMESSENE ENTSCHEIDUNG: `Inhalt` hat KEINE Eigenschaft, die man auf
    /// einen Rand setzen kann — es sind zwei (`width`, `height`) und sie
    /// stehen in zwei Feldern. Also `None` fuer die Seiten, und der Inhalt
    /// traegt Breite und Hoehe getrennt (siehe [`StilAenderung`]).
    pub const fn eigenschaft(self, seite: BoxSeite) -> Option<&'static str> {
        match (self, seite) {
            (BoxEbene::Marge, BoxSeite::Oben) => Some("margin-top"),
            (BoxEbene::Marge, BoxSeite::Rechts) => Some("margin-right"),
            (BoxEbene::Marge, BoxSeite::Unten) => Some("margin-bottom"),
            (BoxEbene::Marge, BoxSeite::Links) => Some("margin-left"),
            (BoxEbene::Rand, BoxSeite::Oben) => Some("border-top-width"),
            (BoxEbene::Rand, BoxSeite::Rechts) => Some("border-right-width"),
            (BoxEbene::Rand, BoxSeite::Unten) => Some("border-bottom-width"),
            (BoxEbene::Rand, BoxSeite::Links) => Some("border-left-width"),
            (BoxEbene::Fuellung, BoxSeite::Oben) => Some("padding-top"),
            (BoxEbene::Fuellung, BoxSeite::Rechts) => Some("padding-right"),
            (BoxEbene::Fuellung, BoxSeite::Unten) => Some("padding-bottom"),
            (BoxEbene::Fuellung, BoxSeite::Links) => Some("padding-left"),
            // GEMESSENE ENTSCHEIDUNG: der Inhalt traegt KEINE Seiten, weil
            // `width` keine Seite hat. Ein Feld „Inhalt links" waere eine
            // Eigenschaft, die es in CSS nicht gibt.
            (BoxEbene::Inhalt, _) => None,
        }
    }
}

/// Die vier Seiten einer Box-Ebene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxSeite {
    /// Oben.
    Oben,
    /// Rechts.
    Rechts,
    /// Unten.
    Unten,
    /// Links.
    Links,
}

impl BoxSeite {
    /// Die vier Seiten in der Reihenfolge des Bildes (oben, rechts, unten,
    /// links — die Reihenfolge, in der ein Kasten beschriftet wird).
    pub const ALLE: [BoxSeite; 4] = [
        BoxSeite::Oben,
        BoxSeite::Rechts,
        BoxSeite::Unten,
        BoxSeite::Links,
    ];

    /// Der Index im Zahlenfeld des [`crate::d2_elemente::Boxmodell`]
    /// (`[oben, rechts, unten, links]` — dort steht es so).
    pub const fn index(self) -> usize {
        match self {
            BoxSeite::Oben => 0,
            BoxSeite::Rechts => 1,
            BoxSeite::Unten => 2,
            BoxSeite::Links => 3,
        }
    }

    /// Kurzname fuer die Spalten der Bearbeitung.
    pub const fn kuerzel(self) -> &'static str {
        match self {
            BoxSeite::Oben => "oben",
            BoxSeite::Rechts => "rechts",
            BoxSeite::Unten => "unten",
            BoxSeite::Links => "links",
        }
    }
}

/// EIN WERT IM BOX-MODELL (eine Zahl plus ihre Herkunft im Zustand).
#[derive(Debug, Clone, PartialEq)]
pub struct BoxWert {
    /// Die Ebene.
    pub ebene: BoxEbene,
    /// Die Seite (`Inhalt` hat keine).
    pub seite: Option<BoxSeite>,
    /// Der Wert in px (berechnet, nicht gesetzt — GEMESSEN: `Berechnet` traegt
    /// die aufgeloesten px, nicht die Angabe aus dem Stylesheet; die steht in
    /// `Boxmodell::text_zeilen`).
    pub px: f32,
    /// Steht in diesem Feld eine Angabe aus dem Stylesheet? (`width: auto`
    /// heisst: nein, und 0 px sind dann eine echte Zahl.)
    pub gesetzt: bool,
}

/// DIE SECHSZEHN (bzw. achtzehn) BOX-WERTE — aus dem berechneten Stil.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine Funktion und kein zweiter Zustand:
/// `Boxmodell` in `d2_elemente.rs` traegt die Zahlen schon (12 Randwerte plus
/// Inhalt). Wer sie zweimal haelt, haelt zwei Zahlen fuer dieselbe Sache.
pub fn box_werte(b: &Berechnet) -> Vec<BoxWert> {
    let m = crate::d2_elemente::Boxmodell::aus(b);
    let mut raus = Vec::with_capacity(18);
    for e in [
        (BoxEbene::Marge, m.marge),
        (BoxEbene::Rand, m.rahmen),
        (BoxEbene::Fuellung, m.fuellung),
    ] {
        for seite in BoxSeite::ALLE {
            let px = e.1[seite.index()];
            raus.push(BoxWert {
                ebene: e.0,
                seite: Some(seite),
                px,
                // GEMESSENE ENTSCHEIDUNG: `auto` ist bei `marge_*`/`rahmen_*`
                // nicht moeglich (der Anfangswert ist 0), und bei `fuellung`
                // ebenso. Also ist `gesetzt` hier nur eine Anzeige: der Wert
                // steht im Stylesheet oder er ist der Standardwert 0.
                gesetzt: px > 0.0,
            });
        }
    }
    raus.push(BoxWert {
        ebene: BoxEbene::Inhalt,
        seite: None,
        px: m.gelegt_inhalt.map(|b| b.0).unwrap_or(m.inhalt_breite),
        gesetzt: m.inhalt_breite > 0.0,
    });
    raus.push(BoxWert {
        ebene: BoxEbene::Inhalt,
        seite: None,
        px: m.gelegt_inhalt.map(|b| b.1).unwrap_or(m.inhalt_hoehe),
        gesetzt: m.inhalt_hoehe > 0.0,
    });
    raus
}

/// DER WERT EINER BESTIMMTEN STELLE — der Weg, mit dem das Bild und der
/// Klick dieselbe Zahl finden.
pub fn box_wert(b: &Berechnet, ebene: BoxEbene, seite: Option<BoxSeite>) -> Option<f32> {
    let werte = box_werte(b);
    werte
        .iter()
        .find(|w| w.ebene == ebene && w.seite == seite)
        .map(|w| w.px)
        .or_else(|| match (ebene, seite) {
            // Der Inhalt ohne Seite: die BREITE zuerst, dann die Hoehe.
            (BoxEbene::Inhalt, None) => box_werte(b).get(12).map(|w| w.px),
            _ => None,
        })
}

/// EINE AENDERUNG, DIE DER AUFRUFER MACHEN MUSS.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein WERT ist und kein `&mut Seite`:
/// `narsil_page::Seite` ist fremd, und diese Crate besitzt sie nicht (das steht
/// so auch in `d7_speicher` bei `Speicher::herkunft`). Also: das Werkzeug
/// sagt, was sich aendern muss, und der Aufrufer macht es. Genau wie beim
/// Loeschen im Speicher-Reiter.
#[derive(Debug, Clone, PartialEq)]
pub struct StilAenderung {
    /// Die CSS-Eigenschaft (Langform, wie in `narsil_css::Eigenschaft`).
    pub eigenschaft: &'static str,
    /// Der neue Wert in px.
    pub px: f32,
    /// Der Knoten, an dem geaendert wird (`0` = keine Angabe).
    pub knoten: u32,
}

impl std::fmt::Display for StilAenderung {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}px", self.eigenschaft, self.px)
    }
}

/// DAS EDITIERFELD FUER EINEN BOX-WERT.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Text im Zustand steht und nicht nur der
/// Zielzeiger: der Nutzer tippt „12" und loescht es zweimal; beides muss
/// zurueckgenommen werden koennen. Wer nur den Zielzeiger haelt, kann den
/// Wert setzen, aber nicht schrittweise — und ein Feld, das beim zweiten
/// Tastendruck springt, ist nicht bedienbar.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoxEingabe {
    /// Welches Feld bearbeitet wird (Klick auf einen Wert).
    pub ziel: Option<(BoxEbene, Option<BoxSeite>)>,
    /// Der getippte Text.
    pub text: String,
    /// Wo der Cursor steht (Zeichen, nicht Byte).
    pub cursor: usize,
    /// Nimmt das Feld die Tastatur an?
    pub fokus: bool,
}

impl BoxEingabe {
    /// Das Feld auf einen Wert setzen (der Klick) — der Text ist der
    /// **gemessene** Wert, nicht ein leeres Feld.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum mit dem Wert anfangen: Firefox tut das
    /// nicht (das Feld ist leer), aber Firefox hat eine Undo-Taste und eine
    /// Historie. Ohne Historie ist ein leeres Feld ein Feld, in dem man die
    /// Zahl tippen muss, die schon dasteht — und ein Tippfehler darin ist
    /// nicht zurueckzunehmen, ausser man klickt woanders hin.
    pub fn waehlen(&mut self, ziel: (BoxEbene, Option<BoxSeite>), px: f32) -> bool {
        if self.ziel == Some(ziel) && self.fokus {
            return false;
        }
        self.ziel = Some(ziel);
        self.text = px_text(px);
        self.cursor = self.text.chars().count();
        self.fokus = true;
        true
    }

    /// Das Feld verlassen.
    pub fn verlassen(&mut self) {
        self.fokus = false;
    }

    /// Ein Zeichen tippen (M4: hoechstens `max_eingabe`).
    pub fn tippen(&mut self, c: char) -> bool {
        let g = Grenzen::STANDARDAUSGABE;
        if self.text.chars().count() >= g.max_eingabe {
            return false;
        }
        let pos = self.cursor.min(self.text.chars().count());
        self.text.insert(pos, c);
        self.cursor = pos + 1;
        true
    }

    /// Backspace.
    pub fn zurueck(&mut self) -> bool {
        if self.cursor == 0 || self.text.is_empty() {
            return false;
        }
        let pos = self.cursor - 1;
        let byte = self
            .text
            .char_indices()
            .nth(pos)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len());
        self.text.remove(byte);
        self.cursor = pos;
        true
    }

    /// Den Text als Zahl lesen — `None`, wenn er keine Zahl ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das so streng ist: `parse::<f32>()`
    /// nimmt auch „12abc"? Nein — es nimmt nur, was eine Zahl ist. Wer
    /// zusaetzlich `px` oder Leerzeichen abstreift, macht aus „1 2" eine 12.
    /// Also: `trim()`, und danach muss die ganze Zeile eine Zahl sein.
    pub fn wert(&self) -> Option<f32> {
        let t = self.text.trim();
        if t.is_empty() {
            return None;
        }
        // „-12.5" ist eine Zahl, „+12" nicht (CSS schreibt es nicht).
        let wert: f32 = t.parse().ok()?;
        if wert.is_finite() { Some(wert) } else { None }
    }

    /// Den getippten Wert als **Aenderung** — oder `None`, wenn das Feld
    /// leer ist, keine Zahl enthaelt oder gar kein Feld offen ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier NICHT gekuerzt wird: die Grenze
    /// `max_eingabe` (Zeichen) ist eine Grenze des Textes; der Wert wird
    /// danach geprueft (Zahl, endlich, nicht negativ bei einer Box).
    pub fn uebernehmen(&mut self, knoten: u32) -> Option<StilAenderung> {
        let (ebene, seite) = self.ziel?;
        let px = self.wert()?;
        if px < 0.0 {
            // GEMESSENE ENTSCHEIDUNG: eine negative Box ist ungueltig; der
            // Reiter sagt es und aendert nichts.
            return None;
        }
        let eigenschaft = match (ebene, seite) {
            (BoxEbene::Inhalt, None) => {
                // Zwei Eigenschaften fuer eine Zahl waere eine Liste; fuer
                // den Inhalt gibt es Breite und Hoehe — also nimmt der
                // Aufrufer die Breite, und die Hoehe ueber denselben Weg mit
                // dem ZIEL `Inhalt` (das Feld traegt beide, siehe
                // `BoxEingabe::breite_und_hoehe`).
                "width"
            }
            (e, Some(s)) => e.eigenschaft(s)?,
            _ => return None,
        };
        Some(StilAenderung {
            eigenschaft,
            px,
            knoten,
        })
    }
}

/// EINE EIGENSCHAFT AUS EINER REGEL (`margin-left: 12px`).
#[derive(Debug, Clone, PartialEq)]
pub struct RegelWert {
    /// Der Name der Eigenschaft — GEMESSEN der **Debug-Name** der fremden
    /// Enum (`MargeLinks`, nicht `margin-left`), weil `narsil_css::Eigenschaft`
    /// keinen `name()` hat (04.10., gemessen an `typen.rs:615` und an
    /// `impl Eigenschaft`: es gibt keins). Also KEIN CSS-Namen zu behaupten.
    /// Gemeldet in `team/BRETT.txt` (04.10., an CSS: bitte
    /// `Eigenschaft::name(self) -> &'static str` mit der CSS-Schreibweise).
    pub eigenschaft: String,
    /// Der Wert, wie er in der Regel steht (gekuerzt).
    pub wert: String,
    /// Ist die Eigenschaft AN? (der Schalter im Bild)
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Schalter im Zustand steht und nicht
    /// nur im Bild: ein ausgeschalteter Wert muss beim Anwenden beruecksichtigt
    /// werden — sonst waere der Schalter eine Farbe ohne Wirkung, und der
    /// Nutzer prueft es an der Seite, nicht im Werkzeug.
    pub an: bool,
}

/// EINE REGEL IM REGAEL-PANEL.
#[derive(Debug, Clone, PartialEq)]
pub struct RegelZeile {
    /// Woher die Regel kommt (`datei.css`; leer = unbekannt).
    pub quelle: String,
    /// In welcher Zeile der Datei sie steht (`None` = unbekannt, siehe der
    /// Modul-Kopf).
    pub zeile: Option<u32>,
    /// Der Selektor als Text (aus der fremden Struktur gebaut).
    pub selektor: String,
    /// Die Eigenschaften der Regel, in Quellreihenfolge.
    pub eigenschaften: Vec<RegelWert>,
}

impl RegelZeile {
    /// Die Herkunft als Text, wie sie im Bild steht: `datei.css:12`, oder
    /// nur die Datei, oder `?`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das `?` heisst und nicht „0": eine
    /// Zeilennummer 0 gibt es nicht (Zeilen sind 1-basiert, wie im
    /// Quelltext-Reiter).
    pub fn herkunft_text(&self) -> String {
        match (self.quelle.trim().is_empty(), self.zeile) {
            (true, None) => String::from("?"),
            (true, Some(z)) => format!("?:{z}"),
            (false, None) => self.quelle.clone(),
            (false, Some(z)) => format!("{}:{}", self.quelle, z),
        }
    }

    /// Die Zeilen, wie sie im Bild stehen (eineje Zeile).
    pub fn text_zeilen(&self) -> Vec<String> {
        let mut raus = vec![format!("{}  {}", self.herkunft_text(), self.selektor)];
        //
        // D19: das Schalterzeichen steht VOR einem Leerzeichen, damit „an"
        // und „aus" an derselben Stelle stehen: `  Farbe: red` und
        // `- Farbe: red`. GEMESSEN: zuerst stand es direkt am Namen
        // (`-Farbe: red`), und der Unterschied war um ein Zeichen zu lesen.
        for e in self.eigenschaften.iter() {
            raus.push(format!(
                "{} {}: {}",
                if e.an { " " } else { "-" },
                e.eigenschaft,
                e.wert
            ));
        }
        raus
    }
}

/// DIE REGELN DES GEWAEHLTEN KNOTENS (das Regeln-Panel).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Regeln {
    /// Die Regeln in Kaskadenreihenfolge (die Reihenfolge, in der sie auf
    /// das Element wirken — die spaetere gewinnt).
    pub zeilen: Vec<RegelZeile>,
    /// Wie viele Regeln es gab, aber nicht mehr sind (M7).
    pub abgeschnitten: u32,
}

impl Regeln {
    /// Leer.
    pub fn leer() -> Self {
        Self::default()
    }

    /// Die Zeilen, wie sie im Bild stehen.
    pub fn text_zeilen(&self) -> Vec<String> {
        let mut raus: Vec<String> = self.zeilen.iter().flat_map(|z| z.text_zeilen()).collect();
        if self.abgeschnitten > 0 {
            raus.push(format!("... {} weitere Regeln", self.abgeschnitten));
        }
        raus
    }

    /// Den Schalter einer Eigenschaft umlegen (der Klick im Bild).
    ///
    /// Liefert `true`, wenn sich etwas geaendert hat. `None`, wenn es die
    /// Regel oder die Eigenschaft nicht gibt — dann passiert NICHTS, und der
    /// Aufrufer weiss es (statt eines Fehlers im Bild).
    pub fn schalten(&mut self, regel: usize, eigenschaft: usize) -> bool {
        let Some(r) = self.zeilen.get_mut(regel) else {
            return false;
        };
        let Some(e) = r.eigenschaften.get_mut(eigenschaft) else {
            return false;
        };
        e.an = !e.an;
        true
    }

    /// Ist die Eigenschaft AN? (der Weg des Bildes und des Aufrufers)
    pub fn eigenschaften_an(&self, regel: usize, eigenschaft: usize) -> bool {
        self.zeilen
            .get(regel)
            .and_then(|r| r.eigenschaften.get(eigenschaft))
            .is_some_and(|e| e.an)
    }

    /// Den Wert einer Eigenschaft setzen (das Editierfeld neben der Regel).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der neue Wert hier landet und im Panel
    /// sichtbar wird: ein bearbeiteter Wert, der erst nach dem Neuladen der
    /// Seite im Bild steht, ist eine Zahl, die man nicht pruefen kann.
    pub fn wert_setzen(&mut self, regel: usize, eigenschaft: usize, wert: &str) -> bool {
        let Some(r) = self.zeilen.get_mut(regel) else {
            return false;
        };
        let Some(e) = r.eigenschaften.get_mut(eigenschaft) else {
            return false;
        };
        let neu = wert.trim();
        if neu.is_empty() || neu.len() > Grenzen::STANDARDAUSGABE.max_kopfwert {
            return false;
        }
        if e.wert == neu {
            return false;
        }
        e.wert = neu.to_string();
        true
    }

    /// Die REGELN ALS TEXT — aber **kein gueltiges CSS**.
    ///
    /// GEMESSENE ENTSCHEIDUNG (04.10.): die Eigenschaftsnamen sind die
    /// Debug-Namen der fremden Enum (`MargeLinks`), weil `narsil-css` keinen
    /// `name()` anbietet. Diese Zeilen sind also eine **Anzeige**, kein Blatt,
    /// das man in die Seite schreiben kann. Der Aufrufer schreibt die
    /// Aenderung direkt (er hat den `Berechnet`-Wert); gemeldet in
    /// `team/BRETT.txt`. Ein Werkzeug, das „CSS" verspricht und keins
    /// erzeugt, waergefuehlt wie eins, das es kann.
    pub fn als_text(&self) -> String {
        let mut raus = String::new();
        for z in self.zeilen.iter() {
            raus.push_str(&format!("{} {{\n", z.selektor));
            for e in z.eigenschaften.iter().filter(|e| e.an) {
                raus.push_str(&format!("  {}: {};\n", e.eigenschaft, e.wert));
            }
            raus.push_str("}\n");
        }
        raus
    }
}

/// D19: DIE REGELN, DIE AUF EINEN KNOTEN TREFFEN — mit echter Selektorpruefung.
///
/// # GEMESSEN, dass das ueberhaupt hier geht (04.10.)
///
/// * `narsil_page::kaskade::El` ist oeffentlich (`kaskade.rs:205`) und hat
///   oeffentliche Felder (`baum`, `id`) — und implementiert
///   `narsil_css::Element`. Damit kann **diese** Crate die Regeln selbst
///   filtern; sie muss den Aufrufer nicht bitten.
/// * `narsil_css::selektor_passt_mit(&Selektor, &E, &Zustand) -> bool` ist
///   oeffentlich (GEMESSEN an den Tests der Crate, `tests/k1b_geschwister.rs`).
///
/// GEMESSENE ENTSCHEIDUNG, warum `&Zustand::KEINER` und nicht der Zustand des
/// Fensters: `narsil-page` rechnet die Seite mit `KEINER` (dort dokumentiert,
/// `kaskade.rs:158`), und ein Panel, das mit einem anderen Zustand filterte,
/// zeigte Regeln, die die Seite nicht benutzt. Also: derselbe Zustand wie
/// beim Rechnen. (Ein spaeterer Wunsch: `Zustand` vom Fenster — BRETT.)
///
/// GEMESSENE ENTSCHEIDUNG, warum die Quelle ein PARAMETER ist: `Stylesheet`
/// traegt keine URL (GEMESSEN an `typen.rs:21`) — der Aufrufer weiss, welches
/// Blatt er gelesen hat. `quellen[i]` gehoert zu `blaetter[i]`; fehlt eine,
/// steht im Bild `?` (das ist ehrlich, und `?` steht nicht fuer „Zeile 0").
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge die Quellreihenfolge ist:
/// fuer Regeln gleicher Spezifitaet gewinnt die spaetere — das ist die
/// Kaskade. Wer hier nach Spezifitaet sortieren wuerde, muesste die
/// `!important`-Faelle und die Herkunft (Autor gegen Browser) auch noch
/// beruecksichtigen; die Kaskade macht das richtig, und sie ist an dieser
/// Stelle nicht meine.
pub fn regeln_fuer(
    baum: &narsil_page::kaskade::CssBaum,
    blaetter: &[Stylesheet],
    knoten: u32,
    quellen: &[String],
) -> Regeln {
    let g = Grenzen::STANDARDAUSGABE;
    let el = narsil_page::kaskade::El { baum, id: knoten };
    let zustand = narsil_css::Zustand::KEINER;
    let mut r = Regeln::leer();
    for (i, blatt) in blaetter.iter().enumerate() {
        let quelle = quellen.get(i).cloned().unwrap_or_default();
        for regel in blatt.regeln.iter() {
            match regel {
                Regel::Stil(s) => {
                    if nicht_trifft(s, &el, &zustand) {
                        continue;
                    }
                    if r.zeilen.len() >= g.max_sichtbare_zeilen {
                        r.abgeschnitten = r.abgeschnitten.saturating_add(1);
                        continue;
                    }
                    r.zeilen.push(regel_zeile(s, &quelle, None));
                }
                Regel::Media { regeln, abfrage } => {
                    for s in regeln.iter() {
                        if nicht_trifft(s, &el, &zustand) {
                            continue;
                        }
                        if r.zeilen.len() >= g.max_sichtbare_zeilen {
                            r.abgeschnitten = r.abgeschnitten.saturating_add(1);
                            continue;
                        }
                        let mut z = regel_zeile(s, &quelle, None);
                        // GEMESSEN: eine Regel aus einem `@media`-Block traegt
                        // ihren Block im Selektor-Text — sonst steht im Bild
                        // `p { … }` ohne den Hinweis, dass sie nur bei einer
                        // bestimmten Breite gilt.
                        z.selektor = format!("@media {} | {}", abfrage_text(abfrage), z.selektor);
                        r.zeilen.push(z);
                    }
                }
                //
                // GEMESSEN: `@supports` traegt seine Bedingung, `@layer` seinen
                // Namen. Beide werden wie eine normale Regel aufgenommen — die
                // Bedingung ist im Selektor-Text NICHT sichtbar, und das ist
                // eine Luecke (im Bild sieht man dann nicht, unter welcher
                // Bedingung die Regel gilt). Sie ist in `team/BRETT.txt`
                // gemeldet; hier wird sie nicht erfunden.
                Regel::Supports { .. } | Regel::Layer { .. } => {
                    for s in regeln_von(regel) {
                        if nicht_trifft(s, &el, &zustand) {
                            continue;
                        }
                        if r.zeilen.len() >= g.max_sichtbare_zeilen {
                            r.abgeschnitten = r.abgeschnitten.saturating_add(1);
                            continue;
                        }
                        r.zeilen.push(regel_zeile(s, &quelle, None));
                    }
                }
                Regel::Import { .. } | Regel::FontFace(_) => {}
            }
        }
    }
    r
}

/// Die Stilregeln, die in einer verschachtelten Regel stehen.
fn regeln_von(regel: &Regel) -> &[narsil_css::StilRegel] {
    match regel {
        Regel::Stil(s) => std::slice::from_ref(s),
        Regel::Media { regeln, .. }
        | Regel::Supports { regeln, .. }
        | Regel::Layer { regeln, .. } => regeln.as_slice(),
        Regel::Import { .. } | Regel::FontFace(_) => &[],
    }
}

/// Trifft keine der Selektoren einer Regel den Knoten?
fn nicht_trifft<E: narsil_css::Element>(
    s: &narsil_css::StilRegel,
    el: &E,
    zustand: &narsil_css::Zustand,
) -> bool {
    !s.selektoren
        .iter()
        .any(|sel| narsil_css::selektor_passt_mit(sel, el, zustand))
}

/// Der Text einer `@media`-Abfrage fuer die Anzeige.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier NICHT `{:?}` steht: `MediaAbfrage`
/// ist eine Struktur aus Listen und Vektoren; ihr Debug-Text waere
/// `MediaAbfrage { und_zeilen: [...], oder_zeilen: [] }` — drei Zeilen
/// Mist fuer eine Angabe, die der Nutzer lesen will. Also: leer heisst
/// `all`, sonst die Merkmale nacheinander.
pub fn abfrage_text(a: &narsil_css::MediaAbfrage) -> String {
    if a.oder.is_empty() {
        // GEMESSEN an `typen.rs:128`: „leere Liste = `@media all`".
        return String::from("all");
    }
    let teile: Vec<String> = a.oder.iter().map(media_und_text).collect();
    teile.join(", ")
}

/// Eine `@media`-Zeile als Text.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier KEIN `{:?}` steht: `MediaUnd` hat
/// sieben Felder (Typ, vier Schranken, Ausrichtung). Ihr Debug-Text waere
/// `MediaUnd { typ_ok: true, min_breite: Some(600.0), … }` — eine Zeile, die
/// aus dem Panel laeuft und die niemand liest. Also nur, was wirklich
/// gesetzt ist.
pub fn media_und_text(m: &narsil_css::MediaUnd) -> String {
    //
    // GEMESSEN (Clippy 05:11): die erste Fassung benutzte
    // `if m.min_breite.is_some() { … m.min_breite.unwrap() }` — also ein
    // `unwrap` NACH der Pruefung. Das ist zwei Fehler in einer Zeile: es
    // fragt zweimal, und `unwrap` paniced, wenn sich die Felder aendern.
    // Also `if let Some(v) = …` — eine Pruefung, kein Panikpfad.
    let mut raus = String::new();
    let mut teil = |txt: &str| {
        if !raus.is_empty() {
            raus.push(' ');
        }
        raus.push_str(txt);
    };
    if let Some(v) = m.min_breite {
        teil(&format!("min-width: {v}px"));
    }
    if let Some(v) = m.max_breite {
        teil(&format!("max-width: {v}px"));
    }
    if let Some(v) = m.min_hoehe {
        teil(&format!("min-height: {v}px"));
    }
    if let Some(v) = m.max_hoehe {
        teil(&format!("max-height: {v}px"));
    }
    if let Some(a) = m.ausrichtung {
        if !raus.is_empty() {
            raus.push(' ');
        }
        //
        // GEMESSEN (04.10.): `narsil_css::typen` ist ein **privates** Modul
        // (`mod typen;` in `lib.rs:42`) und `Ausrichtung` steht nicht in der
        // `pub use`-Liste (Zeile 90) — der Typ ist also von aussen nicht
        // erreichbar, auch nicht per Muster. Was bleibt, ist sein `Debug`-
        // Text, und der ist stabil genug fuer eine Anzeige: „Hoch" und
        // „Quer". Ein falscher Wert waere schlimmer als keiner, also wird
        // Unbekanntes als „quer" nur bei „Quer" geschrieben und sonst
        // wortgetreu uebernommen (gemeldet in `team/BRETT.txt`: `Ausrichtung`
        // und die anderen Typen sollten mit exportiert werden).
        raus.push_str(match format!("{a:?}").as_str() {
            "Hoch" => "portrait",
            "Quer" => "landscape",
            other => other,
        });
    }
    if raus.is_empty() {
        // GEMESSEN: `typ_ok` sagt, ob der Typ erlaubt ist; `false` heisst, die
        // Zeile trifft nie. Das ist im Panel eine Information, kein Fehler.
        if m.typ_ok {
            String::from("all")
        } else {
            String::from("(trifft nie)")
        }
    } else {
        raus
    }
}

/// Eine Regel des Panels aus einer fremden `StilRegel`.
fn regel_zeile(s: &narsil_css::StilRegel, quelle: &str, zeile: Option<u32>) -> RegelZeile {
    let g = Grenzen::STANDARDAUSGABE;
    let selektor = s
        .selektoren
        .iter()
        .map(selektor_text)
        .collect::<Vec<String>>()
        .join(", ");
    RegelZeile {
        quelle: quelle.to_string(),
        zeile,
        // GEMESSENE ENTSCHEIDUNG: der Selektor wird auf `max_zeile` Zeichen
        // gekuerzt — eine Selektorgruppe aus 40 Klassen waere eine Zeile, die
        // aus dem Bild laeuft, und `kuerze_zeile` haengt „..." an.
        selektor: g.kuerze_zeile(&selektor),
        eigenschaften: s
            .deklarationen
            .iter()
            .take(g.max_attribute)
            .map(|d: &Deklaration| RegelWert {
                eigenschaft: format!("{:?}", d.eigenschaft),
                wert: wert_text_d(&d.roh, d.wichtig),
                an: true,
            })
            .collect(),
    }
}

/// D19: EINE ZAHL ALS TEXT (px, ohne ueberfluessige Nachkommastellen).
///
/// GEMESSENE ENTSCHEIDUNG: `{:?}` auf `f32` macht aus 12.0 die Zeichenkette
/// „12.0" und aus 0.30000000000000004 eine noch laengere; beides steht dann
/// im Editierfeld, aus dem der Nutzer den Wert zuruecknimmt. Also wird auf
/// eine Nachkommastelle gekuerzt, und `-0` wird zu `0`.
pub fn px_text(v: f32) -> String {
    let gerundet = (v * 10.0).round() / 10.0;
    if (gerundet - gerundet.round()).abs() < 0.05 {
        return format!("{}", gerundet.round());
    }
    let mut s = format!("{gerundet:.1}");
    if s.ends_with(".0") {
        s.truncate(s.len() - 2);
    }
    s
}

/// Der Wert einer Deklaration als Text.
fn wert_text_d(roh: &str, wichtig: bool) -> String {
    let g = Grenzen::STANDARDAUSGABE;
    let t = if roh.trim().is_empty() {
        String::from("(gesetzt)")
    } else {
        g.kuerze_zeile(roh.trim())
    };
    if wichtig {
        format!("{t} !important")
    } else {
        t
    }
}

/// D19: DER SELEKTOR ALS TEXT.
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier gebaut wird und nicht in
/// `narsil-css`: `Selektor` hat keinen `Display` und keine `text()` — es gibt
/// nur `kette`, `spezifitaet`. Ein Text ist eine **Darstellung**, und
/// Darstellung ist Sache des Werkzeugs.
pub fn selektor_text(s: &Selektor) -> String {
    let mut raus = String::new();
    for (i, glied) in s.kette.iter().enumerate() {
        if i > 0 {
            match glied.komb {
                narsil_css::Kombinator::Nachfahre => raus.push(' '),
                narsil_css::Kombinator::Kind => raus.push_str(" > "),
                narsil_css::Kombinator::Nachbar => raus.push_str(" + "),
                narsil_css::Kombinator::Geschwister => raus.push_str(" ~ "),
                narsil_css::Kombinator::Keiner => {}
            }
        }
        raus.push_str(&einfach_text(&glied.einfach));
    }
    if raus.is_empty() {
        // GEMESSEN: ein leerer Selektor ist ungueltig (`narsil-css` verwirft
        // ihn) — aber eine leere Zeile im Bild waere eine Zeile ohne Inhalt.
        String::from("*")
    } else {
        raus
    }
}

/// Ein Glied des Selektors als Text.
fn einfach_text(e: &narsil_css::Einfach) -> String {
    //
    // GEMESSEN (Bild `devpanel_layout_regeln.png`, 05:08): mit dem
    // Universal-Selektor sah die Regel `.klasse { … }` im Bild als
    // `*.klasse` aus. Das ist eine ANDERE Schreibweise — sie stimmt zwar
    // (der Universal-Selekktor ist optional), aber sie ist nicht die, die
    // im Stylesheet steht. Also: `*` nur, wenn gar nichts anderes dasteht.
    let mut raus = e.tag.clone().unwrap_or_else(|| {
        if e.id.is_some()
            || !e.klassen.is_empty()
            || !e.attribute.is_empty()
            || !e.pseudos.is_empty()
        {
            String::new()
        } else {
            String::from("*")
        }
    });
    if let Some(id) = e.id.as_ref() {
        raus.push('#');
        raus.push_str(id);
    }
    for k in e.klassen.iter() {
        raus.push('.');
        raus.push_str(k);
    }
    for p in e.pseudos.iter() {
        raus.push(':');
        raus.push_str(&format!("{p:?}"));
    }
    raus
}

/// D19: DIE ROLLE EINES KNOTENS (minimal, wie im Reiter von Firefox).
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier steht und nicht in `narsil-page`:
/// es ist eine Abbildung von Tag + Attributen auf ein Wort — eine
/// **Darstellung**. Und sie ist klein genug, um ehrlich zu bleiben: sie
/// behauptet nur, was aus dem Tag folgt, und nimmt `role`/`aria-label` vor,
/// wenn sie da sind.
// ===========================================================================
// BARRIEREFREIHEITS-BAUM (Auftrag der Leitung vom 04.10.)
// ===========================================================================

/// D20: EIN KNOTEN IM BARRIEREFREIHEITS-BAUM.
///
/// # WAS DRIN STEHT (und was nicht)
///
/// GEMESSENE ENTSCHEIDUNG: ein Knoten traegt **Rolle** und **Name** — genau
/// die beiden Angaben, an denen man im Alltag steht („was liest das vor?",
/// „was ist das?"). Er traegt KEINE Liste der Properties (in Firefox gibt es
/// 40 Felder je Knoten: `focusable`, `invalid`, `readonly` …) — die sind
/// groesstenteils „falsch", und eine Liste, die meistens falsch sagt, ist
/// schlimmer als keine.
#[derive(Debug, Clone, PartialEq)]
pub struct A11yKnoten {
    /// Der Knoten im `CssBaum` (`0` = die Wurzel des Baums).
    pub knoten: u32,
    /// Die Tiefe (0 = Wurzel).
    pub tiefe: u32,
    /// Die Rolle (aus dem Tag, `role` hat Vorrang).
    pub rolle: String,
    /// Der Name (aus `aria-label`, `alt`, `value`, `placeholder`, `title`).
    ///
    /// GEMESSENE ENTSCHEIDUNG: leer heisst „die Vorlesesoftware sagt nichts",
    /// und im Bild steht dann `—`. Erfunden wird kein Name (siehe
    /// `Elemente::barrierefreiheit_bauen` — `CssKnoten` hat keinen Text).
    pub name: String,
    /// Hat der Knoten Kinder im Baum?
    pub hat_kinder: bool,
}

impl A11yKnoten {
    /// Die Zeile, wie sie im Bild steht.
    pub fn text(&self) -> String {
        let einzug = "  ".repeat(self.tiefe as usize);
        let name = if self.name.is_empty() {
            "\u{2014}".to_string()
        } else {
            self.name.clone()
        };
        format!("{einzug}{}: {}", self.rolle, name)
    }
}

/// D20: DER BARRIEREFREIHEITS-BAUM — die Liste der Knoten mit Rolle und Name.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine FLACHE Liste ist und kein Baum mit
/// Zeigern: der Zeichencode zeichnet Zeilen, der Klick nennt eine Nummer, und
/// beides braucht keine Verkettung. `tiefe` traegt die Einrueckung — wie in
/// `d2_elemente` (die DOM-Baumzeilen sind auch flach).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct A11yBaum {
    /// Die Knoten in Dokumentreihenfolge.
    pub knoten: Vec<A11yKnoten>,
    /// Welcher Knoten gewaehlt ist (Index in `knoten`).
    pub gewaehlt: Option<usize>,
    /// Wie viele Knoten es gab, aber nicht mehr sind (M7).
    pub abgeschnitten: u32,
}

impl A11yBaum {
    /// Leer.
    pub fn leer() -> Self {
        Self::default()
    }

    /// Die Zeilen, wie sie im Bild stehen.
    pub fn text_zeilen(&self) -> Vec<String> {
        let mut raus: Vec<String> = self.knoten.iter().map(|k| k.text()).collect();
        if self.abgeschnitten > 0 {
            raus.push(format!("... {} weitere Knoten", self.abgeschnitten));
        }
        raus
    }

    /// Den Knoten an einer Nummer waehlen. `true`, wenn sich etwas geaendert
    /// hat.
    pub fn waehle(&mut self, nr: usize) -> bool {
        if nr >= self.knoten.len() {
            return false;
        }
        if self.gewaehlt == Some(nr) {
            return false;
        }
        self.gewaehlt = Some(nr);
        true
    }

    /// Der gewaehlte Knoten.
    pub fn gewaehlt(&self) -> Option<&A11yKnoten> {
        self.gewaehlt.and_then(|i| self.knoten.get(i))
    }

    /// Die Rollen, die im Baum vorkommen, mit ihrer Zahl (die Zusammenfassung
    /// der Kopfzeile).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zusammenfassung hier steht und nicht
    /// im Zeichencode: sie ist eine Rechnung ueber die Liste, und der Zeichencode
    /// darf sie nicht selbst machen (zwei Wege durch dieselben Zeilen).
    pub fn rollen_zaehlung(&self) -> Vec<(String, usize)> {
        let mut raus: Vec<(String, usize)> = Vec::new();
        for k in self.knoten.iter() {
            match raus.iter_mut().find(|(r, _)| r == &k.rolle) {
                Some((_, n)) => *n += 1,
                None => raus.push((k.rolle.clone(), 1)),
            }
        }
        raus.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        raus
    }

    /// Die Kopfzeile: wie viele Knoten, wie viele Rollen, wie viele Namen.
    pub fn kopf_zeile(&self) -> String {
        let mit_name = self.knoten.iter().filter(|k| !k.name.is_empty()).count();
        let mut z = format!(
            "{} Knoten | {} mit Namen{}",
            self.knoten.len(),
            mit_name,
            if self.abgeschnitten > 0 {
                format!(" | {} abgeschnitten", self.abgeschnitten)
            } else {
                String::new()
            }
        );
        let zaehlung = self.rollen_zaehlung();
        if !zaehlung.is_empty() {
            let mut teile: Vec<String> = zaehlung
                .iter()
                .take(6)
                .map(|(r, n)| format!("{r} {n}"))
                .collect();
            if zaehlung.len() > 6 {
                teile.push(format!("... {} Rollen", zaehlung.len()));
            }
            z.push_str(&format!(" | {}", teile.join(", ")));
        }
        z
    }
}

/// D20: DEN BAUM AUS EINER ECHTEN SEITE BAUEN.
///
/// # GEMESSEN, WOHER DIE ANGABEN KOMMEN (04.10.)
///
/// `narsil_page::kaskade::CssBaum` traegt fuer jeden Knoten `tag` und
/// `attribute` (oeffentliche Felder) — und **kein `kinder`** (grep: 0
/// Treffer). Die Kinder stehen deshalb nicht am Baum, sondern im HTML-
/// `document` der Seite (`s.document`), das `narsil`/`narsil::Sitzung` haelt.
///
/// GEMESSENE ENTSCHEIDUNG, warum trotzdem aus dem `CssBaum` gebaut wird und
/// nicht aus `document`: die Rolle und der Name kommen aus Tag + Attributen,
/// und der Tag steht doppelt da (im HTML-Knoten und im CSS-Knoten). Der Baum
/// ist die Kopie, die der Reiter ohnehin schon haelt (`Elemente::baum`), und
/// er traegt die ATTRIBUTE — im HTML-Knoten muesste man sie zuerst
/// zusammensuchen.
pub fn a11y_aus_baum(b: &narsil_page::kaskade::CssBaum, max_knoten: usize) -> A11yBaum {
    // GEMESSEN (04.10., 12:07): `Eigen` ist in DIESER Datei nicht importiert
    // (die anderen Stellen benutzen `Grenzen` direkt) — der erste Entwurf
    // nahm die fremde Kurzform und baute nicht.
    let g = crate::grenzen::Grenzen::STANDARDAUSGABE;
    let mut r = A11yBaum::leer();
    // Die Eltern je Knoten: der Baum traegt `eltern`, also laesst sich die
    // Tiefe ausrechnen, ohne den HTML-Baum zu fragen.
    for (i, k) in b.knoten.iter().enumerate() {
        // Ein Textknoten (leerer Tag) ist kein Knoten fuer die
        // Vorlesesoftware — er wird nicht gezaehlt.
        if k.tag.is_empty() {
            continue;
        }
        if r.knoten.len() >= max_knoten {
            r.abgeschnitten = r.abgeschnitten.saturating_add(1);
            continue;
        }
        let mut tiefe = 0u32;
        let mut eltern = k.eltern;
        while let Some(e) = eltern {
            if tiefe as usize >= g.max_tiefe {
                break;
            }
            tiefe += 1;
            eltern = b.knoten.get(e as usize).and_then(|x| x.eltern);
        }
        let attr = |n: &str| k.attribute.get(n).cloned();
        let mut kinder = Vec::new();
        for (j, x) in b.knoten.iter().enumerate() {
            if x.eltern == Some(i as u32) && !x.tag.is_empty() {
                kinder.push(j);
            }
        }
        r.knoten.push(A11yKnoten {
            knoten: i as u32,
            tiefe,
            rolle: rolle(&k.tag, &attr),
            name: name(&k.tag, &attr, ""),
            hat_kinder: !kinder.is_empty(),
        });
    }
    r
}

/// D19: DIE BARRIEREFREIHEIT DES GEWAEHLTEN KNOTENS
pub fn rolle(tag: &str, attribute: &dyn Fn(&str) -> Option<String>) -> String {
    if let Some(r) = attribute("role") {
        if !r.trim().is_empty() {
            // GEMESSEN: `role="button"` heisst „Button"; der Attributwert ist
            // schon das, was man sagen will.
            return r.trim().to_ascii_lowercase();
        }
    }
    match tag {
        "a" => {
            if attribute("href").is_some() {
                String::from("link")
            } else {
                String::from("generic")
            }
        }
        "button" => String::from("button"),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => String::from("heading"),
        // GEMESSENE ENTSCHEIDUNG: ein `<img>` ist „image", mit oder ohne
        // `alt`. (Der erste Entwurf unterschied die beiden Faelle und gab in
        // beiden Zweigen dasselbe zurueck — das ist kein Unterschied, sondern
        // zwei Zeilen.) Der NAME unterscheidet sie: siehe `name`.
        "img" => String::from("image"),
        "input" => match attribute("type").map(|t| t.to_ascii_lowercase()) {
            Some(ref t) if t == "checkbox" => String::from("checkbox"),
            Some(ref t) if t == "radio" => String::from("radio"),
            Some(ref t) if t == "text" || t == "email" || t == "password" || t == "search" => {
                String::from("textbox")
            }
            Some(ref t) if t == "submit" || t == "button" || t == "reset" => String::from("button"),
            _ => String::from("textbox"),
        },
        "select" => String::from("combobox"),
        "textarea" => String::from("textbox"),
        "nav" => String::from("navigation"),
        "main" => String::from("main"),
        "header" => String::from("banner"),
        "footer" => String::from("contentinfo"),
        "ul" | "ol" => String::from("list"),
        "li" => String::from("listitem"),
        "table" => String::from("table"),
        "p" | "span" | "div" => String::from("generic"),
        "" => String::from("none"),
        other => return other.to_ascii_lowercase(),
    }
}

/// D19: DER NAME EINES KNOTENS (das, was eine Vorlesesoftware vorliest).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge genau so ist: sie ist die
/// des AccName-Algorithmus ( Accessible Name and Description Computation)
/// in der Kuerze: `aria-label` schlaegt `aria-labelledby`, dann der Inhalt
/// (`alt` bei Bildern, `value`/`placeholder` bei Eingaben), dann `title`.
/// Ein leeres Ergebnis wird **als leer gemeldet**, nicht erfunden.
pub fn name(tag: &str, attribute: &dyn Fn(&str) -> Option<String>, text: &str) -> String {
    if let Some(a) = attribute("aria-label") {
        if !a.trim().is_empty() {
            return a.trim().to_string();
        }
    }
    let aus_attribut = match tag {
        "img" | "area" => attribute("alt"),
        "input" => attribute("value").or_else(|| attribute("placeholder")),
        _ => None,
    };
    if let Some(a) = aus_attribut {
        if !a.trim().is_empty() {
            return a.trim().to_string();
        }
    }
    let t = text.split_whitespace().collect::<Vec<&str>>().join(" ");
    if !t.is_empty() {
        return t;
    }
    attribute("title")
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_default()
}
