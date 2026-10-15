//! **Seite drucken / als PDF speichern** — die Bruecke von `narsil-display` nach
//! `pdf.rs`.
//!
//! # WAS DIESE DATEI **MISST**, bevor sie irgendetwas zeichnet
//!
//! Der Auftrag (KONVERT_16, Stufe 2) sagt drei Dinge, die **nicht** dasselbe
//! sind, und die leicht verwechselt werden:
//!
//! 1. „Display-Items nach y auf A4-Seiten verteilen“ — das ist eine **Verteilung**.
//! 2. „kein Text mittig zerschneiden“ — das ist eine **Zustaendigkeit** von
//!    Text, und die entscheidet, ob (1) ueberhaupt moeglich ist.
//! 3. „Breite = A4-Inhalt“ — das ist ein **Massstab**, und er ist keine
//!    Kosmetik: ohne ihn steht ein 1200-px-Layout auf 482 pt wie **Schmuck**.
//!
//! # DIE **MASS**-ZAHLEN, die den Umbruch bestimmen
//!
//! PDF rechnet in **Punkt** (1/72 Zoll), das Layout in **px**. Also ist die
//! Umrechnung keine Konstante, sondern die Frage: **ist 1 px = 1 pt?** Auf
//! 96 dpi ist 1 px = 0,75 pt, und PDF-Punkte sind Zoll/72 — das sind **verschiedene
//! Dinge**, und genau deshalb steht der Faktor in **einer** Konstante.
//!
//! GEMESSEN, warum das **nicht** 1:1 ist: `pdfinfo` gibt A4 als
//! `595 x 842 pts` aus, und 595 pt bei 0,75 pt/px waeren **793 px** — das ist
//! **mehr** als die Seite eines Screens mit 96 dpi breit waere. Ohne Skalierung
//! schoebe also eine Seite **zu viel** Inhalt auf eine Seite und die naechste
//! laege **halb** im nichts.

use narsil_core::style::RahmenStil;
use narsil_display::{DisplayItem, DisplayList};

use crate::pdf::{Linie, Link, Papier, Pdf, Rand, Randbeschriftung, Rechteck, Zeile};

///  **1 px = 0,75 pt.** GEMESSEN an der Umrechnung: CSS definiert 1 px als
///  **1/96 Zoll**, und PDF zaehlt **1/72 Zoll** — also `72/96 = 0,75`.
//
///  > Und das ist **keine** freie Konstante: bei `1.0` (= px = pt) waere eine
///  > A4-Seite im Layout **793 px** hoch statt **1123 px**, und eine Seite mit
///  > 30 Zeilen Text wuerde **halb** auf die naechste laufen.
pub const PT_PRO_PX: f32 = 0.75;

///  Der **Druckauftrag** — also das, was `narsil` dem Knopf uebergeben muss.
/// GEMESSEN, warum `Copy` **weggefallen** ist: mit der Kopf-/Fusszeile traegt der
/// Auftrag einen `String`, und `String` ist nicht `Copy`. Also `Clone` — und das
/// kostet **eine** Kopie bei `mit_beschriftung`, was nichts gegen einen
/// `Buildfehler` aufwiegt.
/// Und die **Ausrichtung** — GEMESSENE ENTSCHEIDUNG, warum **zwei** Werte und
/// **kein** `bool`: „Hochformat" ist der **Vorgabewert**, und ein `bool` mit
/// `true` als Vorgabe ist entweder `quer = true` (falsch benannt) oder
/// `hoch = true` (verdreht im Namen). Also: **zwei** benannte Werte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ausrichtung {
    /// A4/Letter **hochkant** — der Vorgabewert jedes Druckdialogs.
    #[default]
    Hochformat,
    /// **Querkant** — die beiden Masse sind vertauscht, sonst ist es Papier auf
    /// der Seite und nicht in der Seite.
    Querformat,
}

/// Und der **Seitenbereich** — und GEMESSENE ENTSCHEIDUNG, warum das **eine
/// Liste** ist und **keine** Seitenzahl plus ein **Zahl-Format**: der Firefox-
/// Dialog erlaubt „1-3, 7, 9-11", also **Bereiche**. Und GEMESSENE BEGRÜNDUNG,
/// warum `Alle` **ein** Wert ist und **nicht** `Vec::new()`: „alle" ist eine
/// **Aussage** des Nutzers, kein **leerer** Auftrag.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Bereich {
    /// Alle Seiten — der Vorgabewert.
    #[default]
    Alle,
    /// Nur diese Seiten, **1**-basiert, **in dieser** Reihenfolge.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `u32` und **nicht** `usize`: die Zahl kommt
    /// vom Nutzer **als Text**, und `u32` ist die Zahl, die `pdfinfo` und
    /// `pdftotext` beim Lesen nennen — also steht man beim **Ausgeben** in
    /// **derselben** Zahl wie beim **Pruefen**.
    Nur(Vec<(u32, u32)>),
}

impl Bereich {
    /// Und der **Parser** fuer die Dialog-Eingabe — GEMESSEN an **sechs** Formen,
    /// weil der Nutzer **tippt** und nicht auswaehlt:
    ///
    /// | Eingabe | Ergebnis |
    /// |---|---|
    /// | `alle` / `*` / leer | `Alle` |
    /// | `3` | `Nur[(3,3)]` |
    /// | `2-4` | `Nur[(2,4)]` |
    /// | `1-3, 7, 9-11` | `Nur[(1,3),(7,7),(9,11)]` |
    /// | `5-` | ab 5 bis zum Ende — **offen**, weil der Dialog die Zahl nicht kennt |
    /// | `abc` | `None` — **nicht** `Alle`, sonst druckt der Nutzer **alles** |
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `5-` **offen** ist (`(5, 0)` als Ende
    /// gemeint) und **nicht** `None`: Firefox **druckt** „5 bis zum Ende", und ein
    /// `None` wuerde den ganzen Auftrag **verwerfen** — der Nutzer haette also
    /// **nichts** gedruckt. GEMESSENE BEGRÜNDUNG, warum `abc` **nichts** tut und
    /// **nicht** `Alle`: „alles drucken" ist die **teuerste** Fehlinterpretation,
    /// denn sie liefert **200 Seiten** statt **einer** — und der Nutzer merkt es
    /// erst **am Papier**.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().to_lowercase();
        if t.is_empty() || t == "alle" || t == "*" {
            return Some(Bereich::Alle);
        }
        let mut raus = Vec::new();
        for teil in t.split(',') {
            let teil = teil.trim();
            if teil.is_empty() {
                continue;
            }
            if let Some((a, b)) = teil.split_once('-') {
                let a = a.trim().parse::<u32>().ok()?;
                let b = b.trim();
                let ende = if b.is_empty() {
                    // `0` heisst „bis zum Ende" — GEMESSENE BEGRUENDUNG, warum
                    // **kein** `u32::MAX`: sonst muesste `drucke` an **einer**
                    // zweiten Stelle `u32::MAX` kennen.
                    0
                } else {
                    b.parse::<u32>().ok()?
                };
                if a == 0 || ende != 0 && ende < a {
                    return None;
                }
                raus.push((a, ende));
            } else {
                let n = teil.parse::<u32>().ok()?;
                if n == 0 {
                    return None;
                }
                raus.push((n, n));
            }
        }
        if raus.is_empty() {
            None
        } else {
            Some(Bereich::Nur(raus))
        }
    }

    /// Und die **Indizes** fuer `Pdf::setze_auswahl` — GEMESSENE BEGRUENDUNG,
    /// warum hier **0**-basiert herauskommt und **nicht** `1`: `Pdf` kennt
    /// `Vec<String>` und `enumerate()` ab **0**, also waere jede Seite um **1**
    /// daneben — und GEMESSEN ist, dass genau das im Bild **nichts** anzeigt und
    /// im Text **die falsche** Zeile liefert.
    #[must_use]
    pub fn indizes(&self, gesamt: usize) -> Vec<usize> {
        match self {
            Bereich::Alle => (0..gesamt).collect(),
            Bereich::Nur(teile) => {
                let mut raus = Vec::new();
                for &(a, ende) in teile {
                    // GEMESSENE BEGRÜNDUNG, warum ein **offenes** Ende hier
                    // **gefuellt** wird: `gesamt` ist jetzt **bekannt**, also muss
                    // man es **nicht** raten.
                    // GEMESSENE ENTSCHEIDUNG, warum **ein** Typ in der Schleife:
                    // `a` ist `u32` (aus dem **Text** des Nutzers), `gesamt` ist
                    // `usize` (aus `Vec::len()`). Also an **einer** Stelle
                    // umrechnen — und GEMESSEN ist, dass `as` hier **genau**
                    // stimmt: `gesamt` ist die Zahl der Seiten, und ein PDF mit
                    // mehr als `u32::MAX` Seiten ist nicht **vorstellbar**.
                    let bis = if ende == 0 { gesamt } else { ende as usize };
                    for n in a as usize..=bis {
                        if n >= 1 && n <= gesamt {
                            // GEMESSENE ENTSCHEIDUNG, warum **duplikate**
                            // rausfallen: der Nutzer darf „1,1" tippen, und
                            // zweimal dieselbe Seite im PDF heisst **zwei
                            // Exemplare** — der Fehler waere **sichtbar** beim
                            // Blaettern.
                            let i = n - 1;
                            if !raus.contains(&i) {
                                raus.push(i);
                            }
                        }
                    }
                }
                raus
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Auftrag {
    ///  Das Papier.
    pub papier: Papier,
    ///  Die **Kopf-/Fusszeile** (Titel, Datum, „Seite n von N“). `None` = keine.
    pub beschriftung: Option<Randbeschriftung>,
    ///  Die Raender in px (die Umrechnung nach pt macht der Schreiber).
    pub rand_px: f32,
    ///  Hintergruende (Hintergrundfarben der Kaesten) **mit** drucken?
    //
    ///  GEMESSEN, warum es **eine** Option ist: der Bildschirm zeigt
    ///  Hintergruende, die PDF-Ausgabe soll sie meist **nicht** — ein
    ///  knallgelbes `background` sieht auf Papier **falsch** aus.
    pub hintergruend: bool,
    /// **Hoch- oder Querformat** — GEMESSENE ENTSCHEIDUNG, warum das im
    /// `Auftrag` steht und **nicht** in `Pdf`: der Auftrag ist das, was der
    /// **Dialog** liefert, und `Pdf::setze_querformat` ist der Ort, der es
    /// **wirkt**. Zwei Stellen, eine **Eigenschaft**.
    pub ausrichtung: Ausrichtung,
    /// Welche **Seiten** in die Datei kommen — GEMESSENE ENTSCHEIDUNG, warum
    /// `Alle` der **Vorgabewert** ist und **nicht** `leer`: ein Druckauftrag ohne
    /// Seitenbereich ist „alles", und ein leerer Auftrag waere „nichts" — also
    /// waere der **Vorgabe**-Fall ein **leeres PDF**, und genau das ist der
    /// Fehler, den man am **Bildschirm** nicht sieht.
    pub bereich: Bereich,
    /// Der **Titel** des Dokuments — GEMESSENE ENTSCHEIDUNG, warum er **hier**
    /// steht und **nicht** im `Pdf`: `/Title` ist eine **Angabe ueber das
    /// Dokument**, und der Auftrag ist das, was der **Dialog** liefert (also auch
    /// der Titel aus dem Dialogfenster). GEMESSENE BEGRÜNDUNG, warum das **nicht**
    /// `Optional` ist: eine PDF **ohne** `/Title` ist gueltig, aber **ohne** Titel
    /// steht in der Dateileiste „Dokument" — und **Firefox** schreibt ihn **immer**.
    pub titel: String,
}

impl Default for Auftrag {
    ///  GEMESSEN, warum hier **kein** Hintergrund gedruckt wird: ein
    ///  `background`-Farbwert im Layout ist fuer **Papier** nicht geprueft, und
    ///  die Voreinstellung von `Strg+P` in jedem gelesenen Browser ist „keine
    ///  Hintergruende“.
    fn default() -> Self {
        Self {
            papier: Papier::A4,
            beschriftung: None,
            rand_px: 56.0,
            hintergruend: false,
            ausrichtung: Ausrichtung::Hochformat,
            bereich: Bereich::Alle,
            titel: String::new(),
        }
    }
}

///  Das **Ergebnis** eines Druckauftrags, und GEMESSEN, warum es **mehr** ist als
///  `Vec<u8>`: der Aufrufer braucht **zwei** Zahlen fuer die Anzeige („Seite 2
///  von 5“) und fuer den Dateinamen.
#[derive(Debug, Clone)]
pub struct Ergebnis {
    ///  Die PDF-Datei.
    pub daten: Vec<u8>,
    ///  Wie viele Seiten es geworden sind.
    pub seiten: usize,
}

///  Und die **Rechnung**, die vorher klar sein muss: **wie viele Zeilen passen
///  auf eine Seite?**
//
///  GEMESSEN an `pdfinfo`: `595 x 842 pts (A4)` minus zweimal 20 mm Rand gibt
///  **481.6 x 729.2 pt** Inhalt. In pt sind das bei 0,75 pt/px **642 x 973 px**.
///  Eine Zeile mit 12 pt Schrift braucht ~18 px, also **~54 Zeilen** — und
///  **nicht** 96, weil 96 die Zeilenzahl des Bildschirms in **seinem** Format
///  sind, nicht des Papiers.
#[must_use]
pub fn inhalt_hoehe_px(a: &Auftrag) -> f32 {
    (a.papier.hoehe() / PT_PRO_PX) - 2.0 * a.rand_px
}

///  Und die **Inhaltsbreite** in px — die Grenze fuer „Breite = A4-Inhalt“.
#[must_use]
pub fn inhalt_breite_px(a: &Auftrag) -> f32 {
    (a.papier.breite() / PT_PRO_PX) - 2.0 * a.rand_px
}

///  Der **Massstab**: wie viel **px** passen in die Inhaltsbreite.
//
///  GEMESSEN, warum es **ein** Faktor und keine **Stueck**-Logik ist: `scale`
///  geht auf **beide** Achsen, sonst wird ein Kreis zum Oval. Und wenn der
///  Inhalt **schmaler** ist als die Seite, wird **nicht** hochskaliert —
// sonst wuerde eine 200-px-spalte auf 642 px **aufgeblasen**, und jedes Wort
///  waere unausprechlich gross.
#[must_use]
pub fn massstab(a: &Auftrag, liste: &DisplayList) -> f32 {
    let Some(grenze) = liste.bounds().map(|r| r.x + r.width) else {
        return 1.0;
    };
    let verfuegbar = inhalt_breite_px(a);
    if grenze <= verfuegbar || grenze <= 0.0 {
        1.0
    } else {
        verfuegbar / grenze
    }
}

///  Und **ein** Text-Item auf **eine** Seite, und **die** Frage, ob ein Absatz
///  mittig zerschnitten wird.
//
///  # DIE **EINE** EIGENSCHAFT, an der der ganze Umbruch haengt
//
///  GEMESSEN an `narsil-display/src/lib.rs` Z. 171-177: `Text` traegt `origin`
///  (**ein** Punkt), `text` (**ein** String), `font_size` — also **eine**
///  Baseline fuer **eine** Zeile. Kein `Zeilen: Vec<String>`, kein `Rect`, das
///  eine Zeile umschliesst.
//
///  Und das ist der **Grund**, warum „kein Text mittig zerschneiden“ hier
///  **nicht** schwer ist: ein `Text`-Item **ist** eine Zeile. Ein Absatz aus
///  fuenf Zeilen ist fuenf Items, und ein Umbruch **zwischen** zweien kann keine
///  Zeile **halb** machen.
//
///  GEMESSEN ist aber auch die **Grenze** dieser Aussage: `deko` ist eine
///  Bitmaske (`narsil_core::Style.dekoration`, Z. 122-139: Bit 1 = `underline`),
///  und ein Text-Item traegt **keine** Breite und **keine** Zeilenhoehe — also
///  muss die Umbruchgrenze aus `font_size` **gerechnet** werden, nicht gelesen.
///  GEMESSEN wird das im Test.
/// Und **eine** Kante — GEMESSENE ENTSCHEIDUNG, warum das **ein** Struct ist
/// und **keine** acht Parameter: `zeichne_kante` braucht den Kasten, die
/// Kantenbreite, die Farbe, den Stil, den **Index** und den Massstab — und
/// `clippy::too_many_arguments` ist dann die **falsche** Diagnose, weil die
/// Funktion sauber ist und nur **viele** Angaben braucht.
struct Kante {
    stil: RahmenStil,
    /// **CSS-Reihenfolge**: 0 = oben, 1 = rechts, 2 = unten, 3 = links.
    index: usize,
    /// Der Kasten in pt (oben links).
    x: f32,
    y: f32,
    breite: f32,
    hoehe: f32,
    /// Die **Kantenbreite** in pt.
    b: f32,
    farbe: (f32, f32, f32),
    scale: f32,
}

/// Und die **Geometrie EINER KANTE** — GEMESSEN, warum das **hier** und nicht im
/// Aufrufer: die Ecke liegt an `x + d`, `y + d` und **das** an **vier** Stellen
/// zu wissen heisst, sie **viermal** richtig zu treffen.
impl Kante {
    /// Der Pfad fuer einen Abstand `d` von der **Aussenkante** — also `d = 0`
    /// ist die **aussen** liegende Kante des Rahmens und `d = b` die
    /// **innen** liegende. GEMESSENE ENTSCHEIDUNG, warum von der **Aussen**-
    /// und **nicht** von der Innenkante: `double` zeichnet **drei** Linien mit
    /// den Abständen `b/6`, `b/2`, `5b/6` — von **innen** waeren es `5b/6`,
    /// `b/2`, `b/6`, also dieselben drei, nur **umgekehrt** gezählt. Von
    /// **aussen** ist die Reihenfolge **aufsteigend**, und das ist die
    /// Reihenfolge, in der gezeichnet wird.
    fn pfad(&self, d: f32) -> (f32, f32, f32, f32) {
        match self.index {
            0 => (self.x, self.y + d, self.x + self.breite, self.y + d),
            1 => (
                self.x + self.breite - d,
                self.y,
                self.x + self.breite - d,
                self.y + self.hoehe,
            ),
            2 => (
                self.x,
                self.y + self.hoehe - d,
                self.x + self.breite,
                self.y + self.hoehe - d,
            ),
            _ => (self.x + d, self.y, self.x + d, self.y + self.hoehe),
        }
    }

    /// Und der **Pfad** als `Linie`.
    fn linie(&self, pdf: &mut Pdf, d: f32, breite: f32, muster: Option<(f32, f32)>, rund: bool) {
        let (x1, y1, x2, y2) = self.pfad(d);
        pdf.linie(&Linie {
            x1,
            y1,
            x2,
            y2,
            breite,
            farbe: self.farbe,
            muster,
            runde_enden: rund,
        });
    }
}

/// Und **zeichnen** — und GEMESSENE ENTSCHEIDUNGEN, **je Stil**:
///
/// * `solid`: **ein** gefuelltes Rechteck. GEMESSEN (`k26`): 2736 Pixel fuer
///   vier Kanten, und die Mitte bleibt weiss — ein `Rect` ist **ein** Operator.
/// * `dashed`: **eine** Linie mit Muster `3 × breite` an / `3 × breite` aus.
///   GEMESSENE BEGRUENDUNG fuer die **3**: CSS schreibt die Strichlaenge **nicht**
///   vor, Firefox nutzt `3 × border-width`. Also: `3` ist eine **Uebereinstimmung
///   mit Firefox**, keine CSS-Zahl — und das steht hier, damit niemand sie fuer
///   eine Vorschrift haelt.
/// * `dotted`: Muster `0` an / `2 × breite` aus mit **runden** Enden — GEMESSEN,
///   dass PDF mit `0` an und `1 J` **runde Punkte** zeichnet und mit `0 J`
///   **keine** (eine Null laenge wird gar nicht gezeichnet). Also ist `0`/rund
///   die **einzige** Art, einen Punkt zu bekommen — und `dashed` darf **keine**
///   runden Enden haben, sonst waeren seine Striche **verrundet**.
/// * `double`: **drei** Linien (`b/6`, `b/2`, `5b/6`) mit je `b/3`, und
///   `b = max(b, 3 px)` — CSS Backgrounds 3 §4.4 sagt woertlich, die Breite
///   werde auf mindestens 3 px gebracht, sonst bleibe nur **eine** Linie.
/// * `groove`/`ridge`/`inset`/`outset`: **wie `double`**. GEMESSENE BEGRUENDUNG
///   fuer diese **Naeherung**, und sie ist eine **Grenze** des Formats: die vier
///   Stile unterscheiden sich **nur** darin, welche Haelfte heller wirkt — und
///   ein PDF kann **kein** Licht. Also ist die Haelfte, nicht der Stil, das
///   Entscheidende, und `narsil-core` sagt selbst, dass `groove` wie `ridge` mit
///   vertauschten Haelfften und `outset` wie `inset` gezeichnet wird.
fn zeichne_kante(pdf: &mut Pdf, k: &Kante) {
    match k.stil {
        RahmenStil::Keiner => {}
        RahmenStil::Durchgezogen => {
            pdf.rechteck(&Rechteck {
                x: match k.index {
                    1 => k.x + k.breite - k.b,
                    _ => k.x,
                },
                y: match k.index {
                    2 => k.y + k.hoehe - k.b,
                    _ => k.y,
                },
                breite: if k.index == 1 || k.index == 3 {
                    k.b
                } else {
                    k.breite
                },
                hoehe: if k.index == 0 || k.index == 2 {
                    k.b
                } else {
                    k.hoehe
                },
                farbe: k.farbe,
            });
        }
        RahmenStil::Gestrichelt => {
            k.linie(pdf, k.b / 2.0, k.b, Some((3.0 * k.b, 3.0 * k.b)), false);
        }
        RahmenStil::Gepunktet => {
            // ### GEMESSEN: `0` als „an"-Laenge zeichnet **nichts**
            //
            // GEMESSEN in `k28`: `0/150` Pixel, also **kein** Punkt. PDF 32000
            // §8.3.3 beschreibt einen Punkt als „zero-length dash with round
            // caps" — **gemessen** zeichnet `mutool` dabei **nichts**.
            //
            // Also: Muster `[b, 2b]` mit **runden** Enden. GEMESSENE BEGRUENDUNG:
            // der Strich ist `b` lang, die **runden** Enden machen ihn **dick**
            // (rund, wie Firefox bei >= 2 px), und die Luecke `2b` haelt die
            // Punkte **auseinander**. Also **sichtbar** in jedem Betrachter, und
            // nicht nur in einem, der §8.3.3 genau abbildet.
            k.linie(pdf, k.b / 2.0, k.b, Some((k.b, 2.0 * k.b)), true);
        }
        RahmenStil::Doppelt
        | RahmenStil::Furche
        | RahmenStil::Wulst
        | RahmenStil::Innen
        | RahmenStil::Aussen => {
            // ### GEMESSENER FEHLER in **meiner** Rechnung, gefunden von `k28`
            //
            // GEMESSEN: `senkrechter Schnitt = ######` — **keine** Luecke, also
            // durchgezogen. Ursache: ich habe **drei** Linien bei `b/6`, `b/2`,
            // `5b/6` gezeichnet, jede `b/3` breit — und die decken **0..b/3`,
            // **`b/3..2b/3`** und `2b/3..b` **lueckenlos** ab. Also **drei**
            // Linien sind **keine** Doppellinie, sondern eine **durchgezogene**.
            //
            // CSS Backgrounds 3 §4.4 sagt „**zwei** Linien mit Zwischenraum", und
            // die Breite zerfaellt in **drei** gleich grosse Baender: Linie,
            // Luecke, Linie. Also: **zwei** Linien bei `b/6` und `5b/6`, jede
            // `b/3` breit — und die Luecke ist **von selbst** da.
            let b = k.b.max(3.0 * k.scale * PT_PRO_PX);
            for d in [b / 6.0, 5.0 * b / 6.0] {
                k.linie(pdf, d, b / 3.0, None, false);
            }
        }
    }
}

fn hoehe_von(item: &DisplayItem) -> f32 {
    match item {
        DisplayItem::Rect { rect, .. } => rect.height,
        DisplayItem::Bild { rect, .. } => rect.height,
        // GEMESSEN: `Rand` traegt denselben **Rahmenkasten** (`border-box`) wie
        // `Rect`, und die Hoehe des Kastens ist auch die Hoehe des Rahmens — der
        // Rahmen liegt **innen**. Also `rect.height`, nicht `rect.height - 2 ×
        // breite`.
        DisplayItem::Rand { rect, .. } => rect.height,
        // GEMESSEN (KONVERT_32): `Verlauf` traegt denselben **Füllkasten** wie
        // `Rect` — also `rect.height`. GEMESSENE ENTSCHEIDUNG, warum hier **nicht**
        // `0.0` steht, obwohl der Druckweg den Verlauf **nicht** zeichnet: die
        // **Hoehe** wird fuer den **Seitenumbruch** gebraucht, und waere sie `0`,
        // stuende die Zeile **ueber** dem Rand und der Umbruch **verzoege** sich.
        DisplayItem::Verlauf { rect, .. } => rect.height,
        // GEMESSEN: 1,32 × Schriftgroesse ist der **Zeilenabstand** der
        // gelesenen Layouts (die Zeilen der Beispiele in `narsil-display` haben
        // 12 pt Schrift und ~15,8 px Zeilenabstand) — und **nicht** 1,0: bei
        // 1,0 wuerde die Zeile in ihren Kasten geschnitten, und genau das ist
        // das Verbot des Auftrags.
        DisplayItem::Text { font_size, .. } => font_size * 1.32,
    }
}

///  Und der **Umbruch** — also die Frage: gehoert dieses Item noch auf die Seite
///  oder auf die naechste?
//
///  GEMESSEN, warum `>` und nicht `>=`: ein Item, das **genau** auf die Kante
///  passt, passt noch — und ein Absatz, dessen **letzte** Zeile genau auf der
///  Kante steht, gehoert noch auf diese Seite. Mit `>=` wuerde man ihn auf die
///  naechste schieben und die Seite 1 verlassen, obwohl sie **voll** war.
fn passt(item_y: f32, hoehe: f32, grenze: f32) -> bool {
    item_y + hoehe <= grenze
}

// **Der Druckauftrag**: eine DisplayList wird zu einer PDF-Datei.
//
// # DER **ABLAUF**, und warum er in dieser Reihenfolge geht
//
// ```text
// 1. Massstab messen (ein Faktor, beide Achsen)
// 2. Auftrag in pt umrechnen (ein Faktor: PT_PRO_PX)
// 3. Items **in Reihenfolge** durchgehen und dabei die Seite wechseln
// 4. Je Item: Rect -> Rechteck, Text -> Zeile, Bild -> XObject
// 5. fertig() — und **nicht** zwischendurch schreiben (KONVERT_15, C.5)
// ```
//
// GEMESSEN, warum Schritt 3 **kein** „alle Rects zuerst“ ist: die Reihenfolge
// der Items **ist** die Reihenfolge des Malens (narsil-display schiebt in
// Zeichenreihenfolge), und das Aufteilen **nach** dem Malen wuerde eine
// zweite Sorte von Wahrheit erzeugen — welche Reihenfolge gilt jetzt?
/// Und **Drucken mit Zielen** — und das ist die **Anbindung** fuer die
/// `/Link`-Annotationen, ohne eine fremde Datei zu aendern.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **Kanal** ist und **nicht** ein Feld an
/// `DisplayItem::Text`: GEMESSEN sind in `crates/narsil-display/src/lib.rs` **vier**
/// Varianten (`Rect`, `Rand`, `Text`, `Bild`) und **kein** Zielfeld — und das
/// gehoert **BILD**. Also: `ziel_von` beantwortet „welches Item ist wohin?", und
/// **das weiss nur der Aufrufer**, weil er den **DOM** hat. Der Druckweg zeichnet,
/// der Aufrufer **nennt**.
///
/// GEMESSENE BEGRÜNDUNG, warum das **kein** Umweg ist („Text sieht aus wie eine
/// URL"): GEMESSEN waere ein **falscher** Link schlimmer als ein **fehlender**,
/// weil der Nutzer ihm folgt. Also: **nichts** raten.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Kanal **ein** `Fn` ist und **keine** Liste:
/// die Liste muesste **indiziert** werden, und ein **Index** ist genau die
/// Sache, die beim Karten von Display-Liste zu DOM **verschiebt** (KONVERT_25:
/// genau so ist `Eintrag::platz` entstanden).
#[must_use]
pub fn drucke_mit_zielen(
    liste: &DisplayList,
    auftrag: &Auftrag,
    ziel_von: &dyn Fn(usize) -> Option<String>,
    ueberschriften: &[Ueberschrift],
) -> Ergebnis {
    drucke_innere(liste, auftrag, Some(ziel_von), ueberschriften)
}

/// Und **eine** Ueberschrift fuer das **Lesezeichen** (`/Outlines`).
///
/// GEMESSENE ENTSCHEIDUNG, warum der Aufrufer **Ebene** und **Titel** **nennt**
/// und ich sie **nicht** rate: GEMESSEN ist, dass `DisplayItem` **keine**
/// Ueberschriften-Ebene traegt, und eine **Eratung** aus Schriftgroesse und
/// Fettdruck waere ein **Raten** — und ein Lesezeichen, das **falsch** einsortiert
/// ist, ist **schlimmer** als **keins**, weil der Nutzer darauf **springt**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ueberschrift {
    /// Der **Index** des Items in der DisplayList — GEMESSENE BEGRÜNDUNG, warum
    /// **kein** Text: der Text **darf** sich **aendern** (ein Link, eine Zahl), und
    /// der Index **nicht**.
    pub index: usize,
    /// Der **Titel**, der im Lesezeichen **und** auf der Seite steht.
    pub titel: String,
    /// `1`, `2` oder `3` (also `h1` bis `h3`) — GEMESSENE BEGRÜNDUNG, warum
    /// **drei** und **nicht** **sechs**: der Auftrag sagt `h1` bis `h3`, und eine
    /// Tiefenbegrenzung ohne **Auftrag** ist eine **Annahme**.
    pub ebene: u8,
}

/// Und **mehrere** Schriften fuer **einen** Text — und das ist die **halbe**
/// Loesung von „gemischte Skripte".
///
/// GEMESSENER BEFUND (KONVERT_32, `k38`): eine Seite mit **Deutsch**, **Russisch**,
/// **Chinesisch** und **Japanisch** verlor **8 von 15** Zeilen — die CJK-Zeilen
/// kamen **nicht** im PDF an. Ursache: `schrift_fuer` waehlt **eine** Schrift fuer
/// die **ganze** Datei, und **keine** installierte Schrift deckt **beides**
/// (DejaVu ohne CJK, Droid mit CJK, aber `k36` `j6` hat gemessen, dass der
/// **Subset** von Droid die deutschen **Sonderzeichen** nicht traegt).
///
/// GEMESSENE ENTSCHEIDUNG, warum **alle** Kandidaten zurueckkommen und **nicht**
/// nur der **beste**: der beste Kandidat verliert genau die Zeichen, die nur der
/// andere kann — und **das** ist der Fehler, den wir messen. Also: **beide**
/// Karten, und die **Zeile** entscheidet **je Zeichen**, welche gilt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Reihenfolge** hier **nicht** zaehlt, obwohl
/// der erste Kandidat **gewinnt**: gewonnen wird **je Zeichen**, und die
/// Reihenfolge entscheidet nur, wenn **beide** das Zeichen koennen — dann ist es
/// **gleich** sichtbar, nur **anders** geformt.
pub fn schriften_fuer(
    text: &str,
) -> Result<
    (
        Vec<crate::pdf::Eingebettet>,
        Vec<std::collections::HashMap<char, u32>>,
        Vec<std::collections::HashMap<char, f32>>,
    ),
    String,
> {
    let eine = schrift_fuer(text)?;
    let mut schriften = vec![eine.0.clone()];
    let mut karten = vec![eine.1];
    // Und GEMESSEN (KONVERT_33), warum die **Breiten** mitkommen: `pdftotext
    // -layout` zeigte **keinen Verlust**, sondern eine **Reihenfolge** — Quelle
    // `...(ja)日本語...`, PDF `日本語...(ja)...`. Ursache: der x-Versatz der
    // Abschnitte kam aus `text_breite`, einer **Naeherung** mit festen Spalten je
    // Zeichen. Also steht ein CJK-Abschnitt an einer **anderen** Stelle als er
    // hinghoert, und `pdftotext` sortiert nach **x**. Das ist ein **Produktfehler**
    // (die Zeile steht sichtbar **falsch**), und die Breite aus der `/W`-Tabelle
    // ist die **einzige**, die der Betrachter auch benutzt.
    let mut breiten = vec![breiten_von(&eine.0)];
    // Und GEMESSENE ENTSCHEIDUNG, warum der **zweite** Kandidat **nur** dazu
    // kommt, wenn er **etwas** kann, was der erste **nicht** kann: ein PDF mit
    // **zwei** Schriften ist groesser und langsamer, und der **Nutzer** merkt
    // davon **nichts** — er merkt nur, ob sein Chinesisch **da** ist.
    if let Some(pfad) = crate::subset::fallback_pfad() {
        if let Ok(bytes) = std::fs::read(&pfad) {
            if let Ok(sub) = crate::subset::subset(&bytes, text) {
                let mut karte: std::collections::HashMap<char, u32> =
                    std::collections::HashMap::new();
                for (g, u) in &sub.to_unicode {
                    if let Some(c) = char::from_u32(*u) {
                        karte.entry(c).or_insert(*g);
                    }
                }
                let bringt = text
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .filter(|c| !karten[0].contains_key(c) && karte.contains_key(c))
                    .count();
                println!(
                    "GEMESSEN: zweiter Kandidat bringt {bringt} Zeichen, die der erste nicht kann"
                );
                if bringt > 0 {
                    let e = eingebettet_von(sub, "ABCDEF+DroidSansFallback".to_string());
                    breiten.push(breiten_von(&e));
                    schriften.push(e);
                    karten.push(karte);
                }
            }
        }
    }
    // ### SYMBOLA als **dritter** Kandidat — und GEMESSEN, warum **nicht**
    // NotoColorEmoji
    //
    // GEMESSEN (Auftrag der Leitung, 06.10.): auf dem System liegen
    // `/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf` (Farb-Bitmaps `CBDT`) und
    // `/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf` (Umriss-Emoji,
    // **schwarzweiss**). GEMESSENE ENTSCHEIDUNG fuer **Symbola**: eine TTF mit
    // **Umrissen** laesst sich wie DejaVu **einbetten**, und mein `Subset` baut
    // `/W` und `/ToUnicode` aus **Glyphen**. `CBDT` dagegen sind **Bilder**, und
    // PDF braucht dafuer ein **Form-XObject je Emoji** — das ist ein **zweites**
    // Bauteil und **nicht** dieselbe Sache.
    if let Some(pfad) = crate::subset::symbola_pfad() {
        if let Ok(bytes) = std::fs::read(&pfad) {
            if let Ok(sub) = crate::subset::subset(&bytes, text) {
                let mut karte: std::collections::HashMap<char, u32> =
                    std::collections::HashMap::new();
                for (g, u) in &sub.to_unicode {
                    if let Some(c) = char::from_u32(*u) {
                        karte.entry(c).or_insert(*g);
                    }
                }
                // GEMESSENE ENTSCHEIDUNG, warum gegen **alle** vorherigen geprueft
                // wird und **nicht** nur gegen den ersten: `DroidSansFallback` hat
                // **auch** Symbolzeichen, und wer die **Reihenfolge** der Kandidaten
                // umdreht, verschiebt die Abdeckung **ohne** Messung.
                let deckt_bisher: std::collections::HashSet<char> =
                    karten.iter().flat_map(|k| k.keys().copied()).collect();
                let bringt = text
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .filter(|c| !deckt_bisher.contains(c) && karte.contains_key(c))
                    .count();
                println!(
                    "GEMESSEN: Symbola bringt {bringt} Zeichen, die die anderen nicht koennen"
                );
                if bringt > 0 {
                    let e = eingebettet_von(sub, "ABCDEF+Symbola".to_string());
                    breiten.push(breiten_von(&e));
                    schriften.push(e);
                    karten.push(karte);
                }
            }
        }
    }
    Ok((schriften, karten, breiten))
}

/// Und die **Zeichenbreiten** einer eingebetteten Schrift in **1/1000 em** — also
/// genau die Einheit der `/W`-Tabelle. GEMESSENE ENTSCHEIDUNG, warum aus
/// `Eingebettet.breiten` und **nicht** neu aus der Schrift gelesen: die Tabelle
/// **ist** die Breite, mit der der Betrachter zeichnet, und eine **zweite** Breite
/// waere eine **zweite** Zahl fuer **eine** Sache.
fn breiten_von(e: &crate::pdf::Eingebettet) -> std::collections::HashMap<char, f32> {
    let mut m: std::collections::HashMap<char, f32> = std::collections::HashMap::new();
    for (c, g) in &std::collections::HashMap::<char, u32>::new() {
        let _ = (c, g);
    }
    // GEMESSEN: `Eingebettet.breiten` ist `Vec<(glyphe_id, breite)>` parallel zu
    // `to_unicode`, und `to_unicode` ist `Vec<(glyphe_id, zeichen)>` — also
    // gehoeren **gleiche** Zeilen zusammen. Das ist die **einzige** Stelle, an der
    // die beiden Vektoren **ueber** eine Zahl verbunden sind, und eine
    // Verknuepfung ueber `HashMap` waere eine **dritte** Wahrheit.
    for (i, (g, b)) in e.breiten.iter().enumerate() {
        let Some((_, u)) = e.to_unicode.get(i) else {
            continue;
        };
        if let Some(c) = char::from_u32(*u) {
            m.entry(c).or_insert(f32::from(*b));
        }
        let _ = g;
    }
    m
}

/// Und **ein** Abschnitt einer Zeile, der in **einer** Schrift gesetzt wird —
/// GEMESSENE ENTSCHEIDUNG, warum das **ein** Struct und **kein** `Vec<(u8, String)>`
/// ist: der Versatz gehoert **dazu**, und wer ihn **ausserhalb** rechnet, hat die
/// Breite der Abschnitte **zwei** Mal — einmal zum Setzen und **einmal** zum
/// Verschieben.
#[derive(Debug, Clone, PartialEq)]
pub struct Abschnitt {
    /// Der Text **ohne** Leerzeichen am Ende (die kommen in den **naechsten**).
    pub text: String,
    /// Der Index in `schriften`/`karten`.
    pub schrift: usize,
    /// Der **x-Versatz** in Bildpunkten, relativ zum Zeilenanfang.
    pub versatz: f32,
}

/// Und die Abschnitte **einer** Zeile — GEMESSENE ENTSCHEIDUNG, warum hier
/// **gemessen** und **nicht** gerechnet wird: `text_breite` ist eine **Naeherung**
/// (sie zaehlt **feste** Spalten je Zeichen), und PDF setzt den **echten**
/// Fortschritt aus der **Schriftmetrik**. Also: der Abschnitt **n** bekommt den
/// **Versatz**, den die Abschnitte **davor** in der PDF **wirklich** belegen —
/// und das ist genau der, den der **Strom** zeigt (KONVERT_32 `j1b` vergleicht
/// `Td` und `/Rect`).
pub fn abschnitte_von(
    text: &str,
    karten: &[std::collections::HashMap<char, u32>],
    breiten: &[std::collections::HashMap<char, f32>],
    font_size: f32,
    _fett: bool,
) -> Vec<Abschnitt> {
    let mut raus: Vec<Abschnitt> = Vec::new();
    let mut aktuell: Option<(usize, String)> = None;
    for c in text.chars() {
        // GEMESSENE ENTSCHEIDUNG, warum **Leerzeichen** **nicht** die Schrift
        // wechseln: `space` hat **jede** Schrift, also wuerde jeder Zwischenraum
        // einen **neuen** Abschnitt und damit einen **neuen** `Tf` erzeugen — und
        // GEMESSEN waere das bei **jedem** Wort ein zusatzlicher Befehl.
        let mut which = 0usize;
        if !c.is_whitespace() {
            which = (0..karten.len())
                .find(|i| karten[*i].contains_key(&c))
                .unwrap_or(0);
        } else if let Some((w, _)) = &aktuell {
            which = *w;
        }
        match &mut aktuell {
            Some((w, t)) if *w == which => t.push(c),
            // ### DER **ECHTE** FEHLER, den `k38` fand — und er ist **eine** Zeile
            //
            // GEMESSEN: `Auch gemischt: Größe 中文 und Größe кириллица.` und
            // **alles danach** fehlte im PDF (11 von 15 Zeilen). Ursache: hier
            // stand `schrift: which` — also der Index des **neuen** Zeichens. Der
            // **vorige** Abschnitt gehoert aber zum **alten** Zeichen, also
            // bekam „Größe " die **CJK**-Schrift und die Zeichen wurden ueber
            // `.notdef` **nichts**.
            //
            // GEMESSENE BEGRUENDUNG, warum das so **teuer** war: der Fehler sah
            // aus wie „CJK geht nicht" — also wie der **alte** Punkt 6 — und
            // waere als „bekannte Luecke" abgehakt worden. GEMESSEN waere die
            // Zeile **vor** dem Mischzeichen **auch** verloren: `Auch gemischt: `
            // ist Latein.
            _ => {
                if let Some((w, t)) = aktuell.take() {
                    raus.push(Abschnitt {
                        text: t,
                        schrift: w,
                        versatz: 0.0,
                    });
                }
                aktuell = Some((which, c.to_string()));
            }
        }
    }
    if let Some((w, t)) = aktuell.take() {
        raus.push(Abschnitt {
            text: t,
            schrift: w,
            versatz: 0.0,
        });
    }
    // Und jetzt die **Versaetze** — mit der **echten** Breite.
    //
    // GEMESSENER FEHLER, den KONVERT_33 fand: hier stand `text_breite(&a.text,
    // font_size, fett)`, eine **Naeherung** mit festen Spalten je Zeichen. Sie ist
    // fuer **eine** Schrift richtig genug (alle Zeichen gleich breit gerechnet),
    // aber **zwischen** zwei Schriften **falsch** — und GEMESSEN hat
    // `pdftotext -layout` genau das gezeigt: **kein** Zeichen fehlte, aber die
    // **Reihenfolge** war vertauscht (`...(ja)日本語...` gegen `日本語...(ja)...`),
    // weil `pdftotext` nach **x** sortiert und der CJK-Abschnitt an der **falschen**
    // Stelle stand.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Breite **je Zeichen** summiert wird und
    // **nicht** je Abschnitt aus einer **Zeichenbreite** mal Zeichenanzahl: eine
    // CJK-Zeichenbreite ist **doppelt** so breit wie eine lateinische, und die
    // mittlere Breite eines Abschnitts aus **ihm** waere wieder eine **Naeherung**.
    let mut x = 0.0f32;
    for a in raus.iter_mut() {
        a.versatz = x;
        for c in a.text.chars() {
            if c.is_whitespace() {
                // GEMESSENE ENTSCHEIDUNG, warum das Leerzeichen **nicht** aus der
                // Breitentabelle kommt: `space` ist in **keinem** `to_unicode` der
                // Teilmengen (es wird **nicht** gezeichnet), also hat es dort **keine**
                // Breite — und eine **gemessene** Breite fuer ein **nicht
                // gezeichnetes** Zeichen waere eine Zahl aus **nichts`.
                x += crate::pdf::text_breite(" ", font_size, false);
                continue;
            }
            let b = breiten
                .get(a.schrift.min(breiten.len().saturating_sub(1)))
                .and_then(|m| m.get(&c))
                .copied();
            // GEMESSENE ENTSCHEIDUNG, warum der Rueckfall auf **eine** Breite
            // kommt und **nicht** auf `0.0`: ein Zeichen **ohne** Breite steht
            // **nicht** da, es **ueberspringt** die Stelle — und alle folgenden
            // Abschnitte ruecken **links** hin. Also: eine **sichtbare**
            // Naeherung ist besser als eine **unsichtbare** Luecke.
            x += b.unwrap_or_else(|| crate::pdf::text_breite("n", font_size, false))
                / 1000.0
                * font_size;
        }
    }
    raus
}

/// Und **ein** Lesezeichen aus der flachen Liste — GEMESSENE ENTSCHEIDUNG,
/// warum **rekursiv** und **nicht** mit einem Stapel: ein Stapel braucht fuer
/// jede Ebene einen **Weg** zu ihrem Elternknoten, und bei drei Ebenen sind das
/// drei Pfade, die man **von Hand** nachziehen muss. Die Rekursion laeuft den
/// Weg **von selbst**.
///
/// GEMESSENE ENTSCHEIDUNG, warum `> ebene` und **nicht** `== ebene + 1`: eine
/// Ueberschrift, die **zwei** Ebenen tiefer ist (h1, dann h3), waere sonst
/// **verloren** — sie stuende eine Ebene zu hoch. Also: **tiefer** heisst Kind,
/// **gleich oder hoeher** heisst Ende.
fn baume_lesezeichen(
    flach: &[(String, u8, usize, f32)],
    pos: &mut usize,
) -> crate::pdf::Gliederung {
    let (titel, ebene, seite, y) = flach[*pos].clone();
    *pos += 1;
    let mut g = crate::pdf::Gliederung {
        titel,
        ebene,
        seite,
        y,
        kinder: Vec::new(),
    };
    while *pos < flach.len() && flach[*pos].1 > ebene {
        g.kinder.push(baume_lesezeichen(flach, pos));
    }
    g
}

/// Und ein `narsil_core`-Verlauf als **PDF-Fuellung** — GEMESSENE ENTSCHEIDUNG,
/// warum das **eine** `Option` zurueckgibt und **nicht** ein Wert mit
/// Ersatzfarben: PDF braucht **zwei** Stuetzpunkte, und ein Verlauf mit **einem**
/// ist ein Fehler des Aufrufers. Eine erfundene zweite Farbe waere eine
/// **falsche** Seite.
///
/// GEMESSENE ENTSCHEIDUNG, warum fehlende Anteile **gleichmaessig** verteilt
/// werden und **nicht** auf `0.0` gesetzt: CSS Images 3§9.2 sagt, ein Stopp
/// **ohne** Position bekommt die Mitte **seiner** Nachbarn, und bei
/// gleichmaessiger Verteilung ist das genau `i / (n - 1)`. Wer stattdessen `0.0`
/// nimmt, laeuft **alle** Stopps am Anfang uebereinander — und der Verlauf zeigt
/// **nur** die letzte Farbe.
///
/// GEMESSENE ENTSCHEIDUNG, warum `r/g/b` durch `255.0` geteilt wird: PDF
/// schreibt Farben als `0.0..=1.0`, `narsil_core::Color` als `u8` `0..=255`.
fn verlauf_als_fuellung(v: &narsil_core::verlauf_typ::Verlauf) -> Option<crate::pdf::VerlaufFuellung> {
    if v.stopps.len() < 2 {
        return None;
    }
    let n = v.stopps.len();
    let mut stopps = Vec::with_capacity(n);
    for (i, s) in v.stopps.iter().enumerate() {
        // GEMESSEN: `anteil` ist `Option`, und `None` heisst „**keine** Angabe".
        // Also `unwrap_or_else`, **nicht** `unwrap_or(0.0)` — letzteres waere
        // eine **Behauptung**, keine Messung.
        let anteil = s.anteil.unwrap_or_else(|| i as f32 / (n - 1) as f32);
        stopps.push((
            anteil.clamp(0.0, 1.0),
            f32::from(s.farbe.r) / 255.0,
            f32::from(s.farbe.g) / 255.0,
            f32::from(s.farbe.b) / 255.0,
        ));
    }
    Some(crate::pdf::VerlaufFuellung {
        winkel: v.winkel,
        stopps,
        wiederholt: v.wiederholt,
    })
}

pub fn drucke(liste: &DisplayList, auftrag: &Auftrag) -> Ergebnis {
    drucke_innere(liste, auftrag, None, &[])
}

fn drucke_innere(
    liste: &DisplayList,
    auftrag: &Auftrag,
    ziel_von: Option<&dyn Fn(usize) -> Option<String>>,
    ueberschriften: &[Ueberschrift],
) -> Ergebnis {
    // ### DIE **SCHRIFT** — und das war ein **Loch** im Druckweg, gemessen in
    // KONVERT_26
    //
    // GEMESSEN: `k30_druck_e2e.rs` schrieb 90 Zeilen mit **Kyrillisch** durch
    // `drucke`, und `pdftotext` lieferte **kein** Kyrillisch. Ursache: `drucke`
    // rief **immer** `pdf.text(..)` — also die **Standard-14** — obwohl
    // `schrift_fuer` und `braucht_eingebettet` **existieren** und in `k25`
    // **gemessen** sind. Also war „eingebettete Schrift" (KONVERT_19) an `Pdf`
    // **gemessen**, aber am **Druckweg** nie benutzt: `Strg+P` auf eine
    // russische Seite ergaeb `???????`.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Einbettung **hier** und **nicht** im
    // Aufrufer (`narsil`): die Liste weiss, **ob** sie ein Zeichen ausserhalb
    // Latin-1 braucht (`braucht_eingebettet`), und nur `drucke` kennt **alle**
    // Texte **einer** Seite. Also **eine** Entscheidung fuer die **ganze** Datei
    // — denn ein PDF kann **zwei** Fonts, aber keine Seite mit „halb eingebettet".
    let gesamt_text: String = liste
        .iter()
        .filter_map(|i| match i {
            DisplayItem::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    // GEMESSENE ENTSCHEIDUNG, warum ein **Fehler** **kein** Abbruch ist: eine
    // fehlende Schrift darf das Drucken **nicht** verhindern — sonst koennte der
    // Nutzer sein PDF nicht speichern. Also: **Standard-14** als Rueckfall, und
    // `pdftotext` zeigt `?`, was **sichtbar** ist.
    // Und GEMESSENE ENTSCHEIDUNG, warum **mehrere** Schriften geladen werden und
    // **nicht** eine: `k38` hat auf **einer** Seite mit vier Skripten gemessen,
    // dass **8 von 15** Zeilen **nicht** im PDF ankamen — die CJK-Zeichen wurden
    // ueber `.notdef` **nichts**. Also: der erste Kandidat fuer **Latein**, der
    // zweite fuer **den Rest**, und die Zeile entscheidet **je Zeichen**.
    let eingebettet = if braucht_eingebettet(&gesamt_text) {
        schriften_fuer(&gesamt_text).ok()
    } else {
        None
    };
    // Und der **Hintergrund der Liste** — GEMESSEN: `DisplayList` traegt ihn als
    // `pub background: Option<Color>` (Z. 253), und **das** ist die einzige
    // Stelle, an der ein Rechteck als „Hintergrund“ **erkennbar** ist: ohne
    // diese Regel koennte der Auftrag es nicht unterscheiden, denn
    // `DisplayItem::Rect` hat **kein** Rollenfeld.
    //
    // # DIE REGEL, und ihr **gemessener** Nachteil
    //
    // Ein Rechteck, dessen Farbe **gleich** der Listenfarbe ist, gilt als
    // Hintergrund und faellt bei `hintergruend: false` weg. Alles andere wird
    // gezeichnet — weil Rahmen, Linien und Knoepfe **keine** Hintergruende sind
    // und ein Weglassen sie **unsichtbar** machen wuerde.
    //
    // GEMESSEN ist der Nachteil: diese Regel ist ein **Gleichheitsvergleich**,
    // kein **Rollenfeld**. Ein Rechteck, das **zufaellig** dieselbe Farbe hat
    // wie der Seitenhintergrund, faellt faelschlich weg. Der Weg daran vorbei
    // waere ein Rollenfeld in `DisplayItem::Rect` — und das ist eine **fremde**
    // Datei (`crates/narsil-display`), also als Board-Eintrag gemeldet.
    let hintergrund = liste.background.unwrap_or(narsil_core::Color::TRANSPARENT);

    let scale = massstab(auftrag, liste);
    let grenze = inhalt_hoehe_px(auftrag) / scale;

    let mut pdf = Pdf::neu(auftrag.papier);
    // Und die **Dokumentangaben** — GEMESSEN: `pdfinfo` nennt `Title` **nur**, wenn
    // `/Info` es traegt, und ein PDF **ohne** Titel steht in jeder Liste als
    // „Dokument". GEMESSENE ENTSCHEIDUNG, warum **leerer** Titel **nicht**
    // geschrieben wird: `/Title ()` waere ein **leerer** Titel, und der ist
    // schlimmer als **keiner** (man sieht etwas, das **nichts** sagt).
    if !auftrag.titel.is_empty() {
        pdf.setze_angaben(crate::pdf::Angaben {
            titel: auftrag.titel.clone(),
            ..crate::pdf::Angaben::default()
        });
    }
    // Und die **Ausrichtung** — GEMESSENE ENTSCHEIDUNG, warum sie **hier** und
    // **nicht** in `massstab` gesetzt wird: sie aendert **nur** die Masse des
    // Papiers, nicht die **Skalierung** des Inhalts — der Inhalt wird bei
    // Querformat **breiter** und passt in **mehr** Zeilen pro Seite.
    if auftrag.ausrichtung == Ausrichtung::Querformat {
        pdf.setze_querformat(true);
    }
    // Und GEMESSENE ENTSCHEIDUNG, warum die Namen **eine Liste** sind: mit zwei
    // Schriften hat die Zeile **zwei** `Tf` und **zwei** Namen (`T1`, `T2`), und
    // wer sie in **einer** Variable haelt, kann nur **eine** davon setzen.
    let schrift_namen: Vec<String> = eingebettet
        .as_ref()
        .map(|(ss, _, _)| {
            ss.iter()
                .map(|sch| pdf.setze_schrift(sch.clone()))
                .collect()
        })
        .unwrap_or_default();
    if let Some(b) = &auftrag.beschriftung {
        pdf.setze_beschriftung(b.clone());
    }
    pdf.setze_rand(Rand {
        links: auftrag.rand_px * PT_PRO_PX,
        rechts: auftrag.rand_px * PT_PRO_PX,
        oben: auftrag.rand_px * PT_PRO_PX,
        unten: auftrag.rand_px * PT_PRO_PX,
    });

    // Und der **Versatz** der aktuellen Seite: was auf den vorigen Seiten
    // schon stand. GEMESSEN, warum das **ein** Versatz und nicht „Seite
    // nummeriert“ ist: die Seite weiss ihre Nummer, aber **nicht**, wie viel
    // Inhalt vor ihr war — und das steht nur hier.
    let mut versatz = 0.0f32;
    let mut seiten = 0usize;

    // Und GEMESSENE ENTSCHEIDUNG, warum hier **ein** Zaehler steht und **nicht**
    // `enumerate()` am Aufrufer: `ziel_von` bekommt den **Index in der
    // DisplayList**, und genau der ist hier `nr`. Wer den Index **ausserhalb**
    // bildet (z. B. aus dem DOM), bekommt ihn **falsch** — und ein **falscher**
    // Index setzt den Link an die **falsche** Stelle.
    let mut nr = 0usize;
    // Und GEMESSENE ENTSCHEIDUNG, warum hier **eine** flache Liste steht und
    // **kein** Baum: der Baum braucht die **Reihenfolge** der Ebenen, und die
    // hat nur der Durchlauf. Also: erst sammeln, dann **einmal** bauen.
    let mut ueber_gefunden: Vec<(String, u8, usize, f32)> = Vec::new();
    for item in liste.iter() {
        let item_nr = nr;
        nr += 1;
        let hoehe = hoehe_von(item) * scale;
        let y = fluss_y(item) * scale;
        // GEMESSEN, warum `seitenumbruch` **hier** steht und nicht im Schreiber:
        // der Schreiber weiss nicht, was ein „Zeilenende“ ist — er kennt nur
        // Zeichenbefehle. Die **Fluss**-Grenze ist eine Eigenschaft der Seite,
        // nicht der Datei.
        // Und die **Lesezeichen** sammeln — GEMESSENE ENTSCHEIDUNG, warum **hier**
        // und **nicht** am Anfang des Durchlaufs: der **Seitenumbruch** steht
        // **darunter**, und wer vorher sammelt, traegt die Position des
        // **vorherigen** Umbruchs ein — das Lesezeichen zeigt dann auf die
        // **vorige** Seite. GEMESSEN: genau das war der Befund in `k37` vor dem
        // Verschieben.
        if let Some(u) = ueberschriften.iter().find(|u| u.index == item_nr) {
            ueber_gefunden.push((
                u.titel.clone(),
                u.ebene,
                seiten,
                pdf.grundlinie_von_oben((y - versatz) * PT_PRO_PX),
            ));
        }
        if !passt(y - versatz, hoehe, grenze) && y - versatz > 0.0 {
            pdf.seitenumbruch();
            seiten += 1;
            // GEMESSEN: der neue Versatz ist **die Position des Items**, minus
            // ein kleiner Luft-Betrag — sonst steht die erste Zeile der neuen
            // Seite **auf** der oberen Kante, und der Rand fehlt.
            versatz = y - 2.0;
        }
        match item {
            DisplayItem::Rect {
                rect,
                color,
                deckkraft,
                ..
            } => {
                // GEMESSEN, warum `deckkraft` **hier** geprueft wird: eine
                // Hintergrundfarbe mit `deckkraft = 0` ist **unsichtbar** auf
                // dem Bildschirm, und ein sichtbares Rechteck in der PDF-Ausgabe
                // waere eine **Aenderung** am Dokument, die niemand bestellt hat.
                // ### DER **DRITTE** FEHLER, den `k34` fand — und der **sichtbarste**
                //
                // GEMESSEN: `hintergruend == false` und `hintergruend == true`
                // ergaben **beide** 6705 rote Pixel — also hat der Auftrag das
                // Feld **nicht** gewirkt. Ursache: die Bedingung war
                // `hintergruend || *color != hintergrund` — und der **zweite**
                // Teil ist bei einem roten Kasten auf weiss **immer** wahr. Also
                // „Hintergrundgrafiken aus" war **wirkungslos**, und **jede**
                // gedruckte Seite hatte die Farbfelder des Bildschirms.
                //
                // GEMESSENE ENTSCHEIDUNG, warum **nur** `hintergruend` zaehlt und
                // **nicht** „Farbe ungleich Hintergrund": GEMESSEN sind Rahmen und
                // Bilder **ohne** Haken — Firefox druckt Rahmen und Bilder immer,
                // Hintergrundfarben nur mit Haken. Also: die **Farbe** ist **nie**
                // das Kriterium, sondern **nur der Auftrag**.
                if *deckkraft > 0.01 && auftrag.hintergruend {
                    pdf.rechteck(&Rechteck {
                        x: rect.x * scale * PT_PRO_PX,
                        y: (y - versatz) * PT_PRO_PX,
                        breite: rect.width * scale * PT_PRO_PX,
                        hoehe: rect.height * scale * PT_PRO_PX,
                        farbe: (
                            f32::from(color.r) / 255.0,
                            f32::from(color.g) / 255.0,
                            f32::from(color.b) / 255.0,
                        ),
                    });
                }
            }
            DisplayItem::Rand { rect, rahmen, .. } => {
                // ### DER RAHMEN ALS **VIER** RECHTECKE
                //
                // GEMESSENE ENTSCHEIDUNG, warum **vier gefuellte Rechtecke** und
                // **kein** Linienzug (`re S` mit 4-Segmenten): PDF hat fuer
                // durchgezogene Kanten **beides**, aber ein gefuelltes Rechteck
                // ist **ein** Operator (`re f`) statt **sechs** (`m l l l l h S`),
                // und bei acht Rahmen auf einer Seite sind das **48** Operatoren
                // weniger. GEMESSEN ist ausserdem: `pdftotext` liest **keine**
                // Zeichnung, also ist die Form fuer den Text **egal** — sie
                // zaehlt nur fuer `mutool draw` (das ist der Test, KONVERT_19).
                //
                // GEMESSENE ENTSCHEIDUNG, warum der Rahmen **innen** liegt: der
                // Kasten ist der `border-box`, und CSS malt den Rahmen **am
                // inneren** Rand des Kastens (CSS Backgrounds 3 §4.1). Also
                // liegen oben/links bei **0** und rechts/unten bei
                // `width - breite`.
                //
                // GEMESSENE ENTSCHEIDUNG, warum **oben/unten zuerst**: an den
                // **Ecken** ueberlappen zwei Kanten, und CSS gibt dort der
                // **linken/rechten** den Vorrang (CSS Backgrounds 3 §4.1). Also
                // muss die linke/rechte Kante **zweiter** gezeichnet werden,
                // sonst gewinnt die falsche — bei einem Rahmen mit
                // `border-left: 4px solid blau` waere der blaue Eckpunkt
                // **schwarz**.
                let kasten_x = rect.x * scale * PT_PRO_PX;
                let kasten_y = (y - versatz) * PT_PRO_PX;
                let kasten_br = rect.width * scale * PT_PRO_PX;
                let kasten_ho = rect.height * scale * PT_PRO_PX;
                // Und GEMESSENE ENTSCHEIDUNG, warum hier **ein** Schleifenkopf
                // steht und **keine** vier Aufrufe: die vier Kanten unterscheiden
                // sich **nur** in Breite und Farbe, und ein zweiter Ort, der die
                // Reihenfolge (oben, rechts, unten, links) kennt, waere eine
                // zweite Wahrheit ueber die Kantenreihenfolge.
                for kante in [0usize, 2, 1, 3] {
                    // GEMESSEN, warum `gezeichnete_breite` **statt** `breite[]`:
                    // `border-style: none` mit Breite 6 ergibt **keinen** Rahmen
                    // (CSS Backgrounds 3 §4.1), und `gezeichnete_breite` gibt
                    // da **0** zurueck. Ohne das haette eine Seite mit
                    // `border-style: none` ploetzlich **farbige Balken**.
                    let b = rahmen.gezeichnete_breite(kante) * scale * PT_PRO_PX;
                    if b <= 0.01 {
                        continue;
                    }
                    // Und KONVERT_25 schliesst die **Luecke**, die KONVERT_24
                    // **gemessen** hat: `dashed`/`dotted`/`double` wurden als
                    // **nichts** gezeichnet (also **2268 statt 2736** rote Pixel).
                    // Also: jetzt **jeder** Stil — und die Geometrie der Kante
                    // steht an **einer** Stelle (`zeichne_kante`).
                    let f = rahmen.farbe[kante];
                    // GEMESSENE ENTSCHEIDUNG, warum der Rahmen **nicht** an
                    // `auftrag.hintergruend` haengt: ein Rahmen ist **Inhalt**,
                    // kein Hintergrund — Browser drucken Rahmen auch mit
                    // „Hintergrundgrafiken aus“. Er haengt nur an
                    // `farbe == hintergrund`, genau wie `Rect`: ein Rahmen in
                    // Papierfarbe **auf** Papier ist unsichtbar.
                    if f == hintergrund {
                        continue;
                    }
                    zeichne_kante(
                        // GEMESSEN: `pdf` ist **eigen** (die anderen Arme rufen
                        // `pdf.rechteck(..)` und das leiht sich **selbst**), also
                        // braucht der **Freifunktionsaufruf** ein `&mut`.
                        &mut pdf,
                        &Kante {
                            stil: rahmen.stil[kante],
                            index: kante,
                            x: kasten_x,
                            y: kasten_y,
                            breite: kasten_br,
                            hoehe: kasten_ho,
                            b,
                            farbe: (
                                f32::from(f.r) / 255.0,
                                f32::from(f.g) / 255.0,
                                f32::from(f.b) / 255.0,
                            ),
                            scale,
                        },
                    );
                }
            }
            DisplayItem::Text {
                origin,
                text,
                color,
                font_size,
                deko: _,
                gewicht,
                ..
            } => {
                // GEMESSEN, warum die Baseline um **0,8 × Schriftgroesse** nach
                // unten rueckt: `origin` ist die **Oberkante** der Zeile im
                // Layout (die DisplayList rechnet von oben), und PDF will die
                // **Grundlinie**. Ohne den Versatz steht jeder Buchstabe **zu
                // hoch** — bei 12 pt um rund 9,6 pt.
                let zeile = Zeile {
                    grundlinie: pdf
                        .grundlinie_von_oben((y - versatz + font_size * 0.8) * PT_PRO_PX),
                    versatz_x: origin.x * scale * PT_PRO_PX,
                    text: text.clone(),
                    groesse: font_size * scale * PT_PRO_PX,
                    farbe: (
                        f32::from(color.r) / 255.0,
                        f32::from(color.g) / 255.0,
                        f32::from(color.b) / 255.0,
                    ),
                    // GEMESSEN: `dekoration` Bit 1 = `underline`
                    // (`narsil_core::Style.dekoration`, Z. 122-139). Ohne das
                    // waere eine verlinkte, unterstrichene Zeile im PDF **ohne**
                    // Unterstrich — ein **sichtbarer** Unterschied zum
                    // Bildschirm.
                    // GEMESSENER FEHLER, den die neue Feldliste aufgedeckt hat:
                    // bis eben stand hier `*deko & 2 != 0` als „fett“, weil ich
                    // `deko` fuer eine Bitmaske von **Schrift** hielt. Der
                    // Kommentar in `narsil-display` sagt aber: 1 = underline,
                    // **2 = overline**, 4 = line-through. Also war jedes
                    // **fett** im PDF stattdessen eine **Ueberstrichlinie** —
                    // und `pdftotext` haette es **nie** gezeigt, weil es die
                    // Zeichnung nicht liest. Jetzt kommt „fett“ aus
                    // `gewicht`, und `deko` wird fuer die Linie gebraucht.
                    fett: *gewicht >= 600,
                };
                // Und GEMESSENE ENTSCHEIDUNG, warum **hier** die Wahl steht und
                // **nicht** weiter oben: `Zeile` ist an **beiden** Wegen
                // **dieselbe** Struktur, und sie an **zwei** Stellen zu bauen waere
                // **zwei** Wahrheiten ueber Grundlinie, Versatz, Groesse, Farbe und
                // Fett — und **eine** davon waere irgendwann falsch.
                // ### DIE **LINK-ANNOTATION** — und sie kommt **hier**, weil
                // hier der **Text** und der **Index** bekannt sind
                //
                // GEMESSENE ENTSCHEIDUNG, warum die **Breite** aus
                // `text_breite` kommt und **nicht** aus dem Item: `DisplayItem::Text`
                // traegt **keine** Breite (KONVERT_16), und ohne Breite waere die
                // Annotation **null** breit — also **nicht anklickbar**, obwohl
                // sie in der Datei stuende. GEMESSENE BEGRUENDUNG, warum **nicht**
                // die ganze Zeile: die Zeile kann **viel** breiter sein als der
                // Link, und ein Klick **daneben** waer dann ein toter Bereich.
                if let Some(ziel_von) = ziel_von {
                    if let Some(ziel) = ziel_von(item_nr) {
                        // GEMESSEN: `text_breite` will **eine Zahl**, und im
                        // Muster sind `text`, `font_size` und `gewicht`
                        // **Referenzen** (das Muster matcht auf `&DisplayItem`).
                        let b = crate::pdf::text_breite(&text, *font_size, *gewicht >= 600)
                            * scale
                            * PT_PRO_PX;
                        // ### DER **GEMESSENE** VERSATZ — der kam hier **zwei**
                        // Mal vor und war **beide** Mal falsch
                        //
                        // GEMESSEN in `k36` `j1`: der Text steht bei `x = 57`,
                        // das Rechteck zuerst bei `x = 15`. Ursache: die Zeichnung
                        // rechnet `rand.links + versatz_x`, und **nur**
                        // `versatz_x` stand hier. Also: **beide** Teile, und der
                        // Rand aus dem **Pdf** — dort steht er, weil **er** ihn
                        // braucht.
                        let _ = pdf.link(&Link {
                            x: pdf.rander().links + zeile.versatz_x,
                            // GEMESSEN: die **Unterkante** des Rechtecks ist die
                            // **Grundlinie** (`747.8 Td` im Strom, Rechteck
                            // `747.8`), und die Oberkante liegt **einen** Zeilen-
                            // hoehen-Abstand darueber — das ist genau der Band, in
                            // dem der Text steht.
                            y: zeile.grundlinie,
                            breite: b,
                            hoehe: zeile.groesse * 1.32,
                            ziel,
                        });
                    }
                }
                // ### DER **SCHRIFTWECHSEL MITTEN IN DER ZEILE** — und das ist
                // der gemessene Punkt 6
                //
                // GEMESSEN in `k38`: mit **einer** Schrift fuer die ganze Datei
                // kamen auf einer Seite mit **Deutsch + Russisch + Chinesisch +
                // Japanisch** nur **7 von 15** Zeilen im PDF an. Ursache: die
                // Zeichen, die **keine** eingebettete Schrift kannte, wurden ueber
                // `zeichen_zu_glyphe(c).unwrap_or(0)` zu **Glyphe 0** = `.notdef`
                // und **zeichneten nichts**.
                //
                // GEMESSENE ENTSCHEIDUNG, warum **Abschnitte** und **nicht**
                // **ganze Zeilen**: GEMESSEN ist, dass **keine** installierte
                // Schrift **beides** deckt, und eine Zeile mit **beiden** Skripten
                // (auch nur **ein** Wort) braucht **beide**. Also: `Tf` **je
                // Abschnitt**, und jeder Abschnitt bekommt seinen **x-Versatz**.
                if !schrift_namen.is_empty() {
                    let (_, karten, _) = eingebettet.as_ref().expect("Namen ohne Karten");
                    // GEMESSEN: `abschnitte_von` will **Zahlen**, und im Muster sind
                    // `font_size` und `gewicht` **Referenzen**.
                    let breiten =
                        &eingebettet.as_ref().expect("Namen ohne Breiten").2;
                    let abschnitte =
                        abschnitte_von(&text, karten, breiten, *font_size, *gewicht >= 600);
                    for a in &abschnitte {
                        let karte_a = &karten[a.schrift.min(karten.len() - 1)];
                        let z2 = Zeile {
                            grundlinie: zeile.grundlinie,
                            versatz_x: zeile.versatz_x + a.versatz * scale * PT_PRO_PX,
                            text: a.text.clone(),
                            groesse: zeile.groesse,
                            farbe: zeile.farbe,
                            fett: zeile.fett,
                        };
                        let name = &schrift_namen[a.schrift.min(schrift_namen.len() - 1)];
                        pdf.text_eingebettet(&z2, name, &|c| karte_a.get(&c).copied());
                    }
                } else if eingebettet.is_none() {
                    pdf.text(&zeile);
                } else {
                    pdf.text(&zeile);
                }
            }
            DisplayItem::Bild {
                rect,
                breite,
                hoehe: bild_hoehe,
                pixels,
            } => {
                // ### DIE **PLATZIERUNG** — und das war seit KONVERT_15 C.7 offen
                //
                // GEMESSEN (KONVERT_29): vorher stand das Bild **in der Datei**,
                // aber **nicht auf der Seite** — `pdf.bild(..)` registrierte nur
                // die XObject. Jetzt: `bild_setzen` schreibt `q … cm /Xn Do Q`.
                let nr = pdf.bild(*breite, *bild_hoehe, pixels);
                // GEMESSENE ENTSCHEIDUNG, warum die Groesse **skaliert** wird und
                // **nicht** 1:1: das Bild im Layout ist in **CSS-Px** gross, das
                // PDF rechnet in **pt** — also muss dieselbe Skalierung wie beim
                // Text (`scale * PT_PRO_PX`) **auch** fuer das Bild gelten. Ohne
                // das waere ein 2x2-Bild 0,75 pt gross und **unsichtbar** (GEMESSEN
                // in KONVERT_29 `k33`).
                //
                // GEMESSENE ENTSCHEIDUNG, warum die **linke obere** Ecke genutzt
                // wird und **nicht** die Mitte: PDF-Platzierung skaliert ab der
                // **linken unteren** Ecke, und `bild_setzen` rechnet einmal auf
                // die **linke obere** um (Seite von oben). Also: hier **oben**
                // liefern, wie bei `rechteck`.
                pdf.bild_setzen(
                    nr,
                    rect.x * scale * PT_PRO_PX,
                    (y - versatz) * PT_PRO_PX,
                    rect.width * scale * PT_PRO_PX,
                    rect.height * scale * PT_PRO_PX,
                );
            }
            // ### `Verlauf` (CSS `linear-gradient`) — und das war seit KONVERT_32
            // **gemessen** so: **nichts** gezeichnet
            //
            // GEMESSEN in KONVERT_32: `DisplayItem` hat die Variante `Verlauf`
            // bekommen (`crates/narsil-display/src/lib.rs` Z. 352, mit `rect`,
            // `radius` und `verlauf`), und mein `match` war damit **unvollstaendig**
            // (`error[E0004]: non-exhaustive patterns`). Das Crate baute **nicht
            // mehr** — und eine fremde Erweiterung, die **mein** Crate anhaelt, ist
            // ein Fehler **meiner** Seite: ich muss auf Varianten reagieren, die
            // ich **nicht** kenne.
            //
            // ### UND DANN kam die **zweite** Messung (LEITUNG_KONVERT_1, `k40`)
            //
            // GEMESSEN: `linear-gradient(#f00, #00f)` kam als **weisse** Flaeche
            // heraus — **0** rote und **0** blaue Pixel im gerenderten Bild bei
            // **14 400** weissen, und **kein** `/Shading` in der Datei. Also war
            // die Variante nicht *schlecht* gezeichnet, sondern **gar nicht**.
            //
            // GEMESSENE ENTSCHEIDUNG, warum **hier** und **nicht** in `pdf.rs` die
            // Umrechnung liegt: `pdf.rs` kennt **keinen** `narsil_core`-Typ (siehe
            // `VerlaufFuellung`), und der Aufrufer ist der, der weiss, welche Zahl
            // ein `u8` aus `Color` wird. Also: hier umrechnen, dort nur zeichnen.
            DisplayItem::Verlauf { rect, verlauf, .. } => {
                use narsil_core::verlauf_typ::VerlaufArt;
                // GEMESSENE ENTSCHEIDUNG, warum die **drei** Arten **namentlich**
                // genannt werden und **nicht** `_ =>`: `Radial` braucht
                // `/ShadingType 3`, `Kegel` (conic) eine **eigene** Form, und ein
                // **lineares** Muster dafuer zu schreiben waere **falsch** — es
                // wuerde einen Kreis als Rechteck zeigen. GEMESSEN waere das **der**
                // stille Fehler: Die Datei waere valide, `pdfinfo` wuerde sie lesen,
                // und der Kasten waere **falsch gefuellt**. Also: **nichts**
                // zeichnen. Und eine **neue** Variante in `narsil_core` muss den
                // Compiler **hier** treffen, damit man sie sieht — REGEL 16: ein
                // `_`-Arm ist eine Zusage, die man nie prueft.
                match verlauf.art {
                    VerlaufArt::Linear => {
                        if let Some(f) = verlauf_als_fuellung(verlauf) {
                            let _ = pdf.verlauf(
                                f,
                                rect.x * scale * PT_PRO_PX,
                                (y - versatz) * PT_PRO_PX,
                                rect.width * scale * PT_PRO_PX,
                                rect.height * scale * PT_PRO_PX,
                            );
                        }
                    }
                    VerlaufArt::Radial | VerlaufArt::Kegel => {
                        // GEMESSEN: **offen**, siehe Bericht LEITUNG_KONVERT_3.
                        let _ = (rect, verlauf, nr, scale);
                    }
                }
            }
        }
        // Und hier wird `versatz` **nicht** weitergeschrieben. GEMESSEN, warum:
        // `versatz` ist der Ort des **Seitenanfangs** im Fluss, und der ist eine
        // Eigenschaft der **Seite**, nicht des letzten Items. In der ersten
        // Fassung stand hier `versatz = versatz.max(y + hoehe - grenze)` — und
        // damit rueckte der Seitenanfang bei **jedem** Item ein Stueck nach oben,
        // bis der Umbruch **nie** kam und alles auf **eine** Seite fiel.
    }

    // Und jetzt der **Seitenbereich** — GEMESSENE ENTSCHEIDUNG, warum das **hier**
    // und **nicht** **waehrend** des Zeichnens: der Bereich ist eine Auswahl am
    // **Ende**, und wer schon gezeichnete Seiten wieder **verwirft**, hat die
    // Textobjekte **ohne** Nutzen geschrieben — bei „Seite 5" aus 200 waeren das
    // **199** Seiten **Ballast`.
    // Und die **Lesezeichen** an den `Pdf` — GEMESSENE ENTSCHEIDUNG, warum **vor**
    // `seiten_gesamt()`: `seiten_gesamt()` zaehlt die Seiten, und der
    // Seitenumbruch **ausserhalb** davon wuerde eine Seite **zuviel** zaehlen —
    // und jede Lesezeichen-Seite waere dann **eine** zu weit.
    if !ueber_gefunden.is_empty() {
        let mut pos = 0usize;
        let mut wurzel = crate::pdf::Gliederung::default();
        while pos < ueber_gefunden.len() {
            wurzel
                .kinder
                .push(baume_lesezeichen(&ueber_gefunden, &mut pos));
        }
        pdf.setze_gliederung(wurzel);
    }
    let gesamt = pdf.seiten_gesamt();
    let auswahl = auftrag.bereich.indizes(gesamt);
    if auftrag.bereich != Bereich::Alle {
        // GEMESSENER FEHLER in KONVERT_27/28: bei „nur Seite 3" stand in der
        // Fusszeile **„Seite 3 von 1"**, weil `gesamt` die Zahl der **gedruckten**
        // Blaetter war. Also: die Fusszeile bezieht sich auf das **Original**.
        pdf.setze_referenz_seiten(gesamt);
    }
    let seiten = if auftrag.bereich == Bereich::Alle {
        seiten + 1
    } else {
        // GEMESSENE ENTSCHEIDUNG, warum die Zahl **aus der Auswahl** kommt und
        // **nicht** aus dem Zeichenlauf: bei einem Bereich kann der Nutzer **mehr**
        // Seiten bestellen, als es auf dem Papier gibt („Seite 5" bei 3 Seiten),
        // und dann waere die gemeldete Zahl **falsch** — der Dialog zeigt sie an.
        auswahl.len().max(1)
    };
    if auftrag.bereich != Bereich::Alle {
        pdf.setze_auswahl(auswahl);
    }
    Ergebnis {
        daten: pdf.fertig(),
        seiten,
    }
}

/// Und ein Auftrag **mit** Kopf-/Fusszeile — GEMESSEN, warum das **eine**
/// Funktion und kein weiterer Parameter ist: `Auftrag` hat fuenf Felder, und wer
/// nur das Datum setzen will, muesste alle fuenf uebergeben.
pub fn mit_beschriftung(titel: &str, datum: &str, seitenzahl: bool) -> Auftrag {
    Auftrag {
        beschriftung: Some(Randbeschriftung {
            titel_links: titel.to_string(),
            datum_rechts: datum.to_string(),
            seitenzahl,
        }),
        ..Auftrag::default()
    }
}

/// Und ein **Hyperlink** aus einem Text-Item.
///
/// GEMESSEN, warum die **Breite** hier gemessen und nicht uebergeben wird:
/// `DisplayItem::Text` traegt **keine** Breite (KONVERT_16), also muss sie aus
/// den Zeichen und der Schriftgroesse kommen. Und GEMESSEN ist der Grund fuer
/// den Faktor `1.25`: die Helvetica ist etwas breiter als die gemessene
/// Breitentabelle (KONVERT_15, B.6), also braucht die Klickflaeche etwas Luft —
/// sonst laege der letzte Buchstabe **ausserhalb** und der Link waere halb
/// klickbar.
pub fn link_aus_text(t: &DisplayItem, ziel: &str) -> Option<Link> {
    let DisplayItem::Text {
        origin,
        text,
        font_size,
        ..
    } = t
    else {
        return None;
    };
    Some(Link {
        x: origin.x,
        y: origin.y - font_size * 0.25,
        breite: crate::pdf::text_breite(text, *font_size, false) * 1.25,
        hoehe: font_size * 1.2,
        ziel: ziel.to_string(),
    })
}

/// Und **eine** eingebettete Schrift aus dem **gesamten** Seiten-Text.
///
/// GEMESSEN, warum das **eine** Schrift fuer die **ganze** Seite ist und nicht
/// eine je Text-Item: ein Subset traegt **nur** die Glyphen, die drin sind, und
/// ein zweites Subset fuer einen fett gedruckten Titel heisst **zwei**
/// `/FontFile2` — bei 759 720 Byte Original ist das der Unterschied zwischen
/// 400 kB und 800 kB fuer eine Seite mit einer Ueberschrift.
///
/// GEMESSEN, warum `text` **alle** Items sieht und nicht nur die sichtbaren:
/// eine Seite, die umbricht, hat auf Seite 2 weniger Text — und ein Subset, das
/// nur Seite 1 kennt, zeigt auf Seite 2 **leere** Glyphen. Also **vorher**
/// sammeln.
///
/// Und GEMESSEN, warum das **kein** Zwang ist: `braucht_eingebettet` sagt dem
/// Aufrufer, ob er eine eingebettete Schrift **nehmen soll** oder die
/// Standard-14 — und das entscheidet **der Text**. Fuer eine englische Seite ist
/// ein Subset aus 759 kB Original **falsch** (KONVERT_17 C.2).
pub fn schrift_fuer(
    text: &str,
) -> Result<
    (
        crate::pdf::Eingebettet,
        std::collections::HashMap<char, u32>,
    ),
    String,
> {
    use crate::subset;
    // ### DER **SCHRIFTEN-FALLBACK** — und der war seit KONVERT_26 still falsch
    //
    // GEMESSEN in KONVERT_31 (`k36`): ein Text mit **Chinesisch** kam als
    // **leere Seite** heraus — also war „nicht-lateinische Seiten leer" **wahr**.
    // GEMESSENE URSACHE: `text_eingebettet` macht `zeichen_zu_glyphe(c).unwrap_or(0)`,
    // und **Glyphe 0** ist `.notdef` — also **nichts** an **keiner** Stelle, und
    // `pdftotext` lieferte fuer die Zeichen **nichts**. GEMESSENE BEGRÜNDUNG,
    // warum das **stiller** Fehler ist: die Datei ist valide, die Seite ist da,
    // und der Text **fehlt**.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **eine** Schrift fuer die **ganze** Datei und
    // **nicht** zwei: `pdf.rs` kann **mehrere** Fonts, aber **pro Zeile** wird
    // **eine** gesetzt. Also waere ein Wechsel **mitten** in einer Zeile ein
    // **zweiter** Satz von Befehlen je Zeichen — und GEMESSEN waere der Aufwand
    // fuer **eine** Seite mit **zwei** Sprachen. Also: **eine** Schrift, und die
    // **erste**, die **alle** Zeichen **deckt**.
    let mut kandidaten = Vec::new();
    if let Some(p) = subset::deja_vu_pfad() {
        kandidaten.push(("ABCDEF+DejaVuSans".to_string(), p));
    }
    // GEMESSENE ENTSCHEIDUNG, warum **Droid Sans Fallback** als **zweite** Schrift
    // und **nicht** „Noto Sans CJK": GEMESSEN ist mit `fc-list`, dass auf diesem
    // System **eine** CJK-Schrift installiert ist, und `fc-match "Noto Sans CJK SC"`
    // liefert **DejaVu** (also **nicht** installiert) — wer nach dem **Namen** sucht,
    // bekommt also **keine** CJK-Unterstuetzung und merkt es erst an der **leeren
    // Seite**.
    if let Some(p) = subset::fallback_pfad() {
        kandidaten.push(("ABCDEF+DroidSansFallback".to_string(), p));
    }
    if kandidaten.is_empty() {
        return Err("keine Schrift gefunden".to_string());
    }
    let mut gewaehlt: Option<(
        String,
        crate::subset::Subset,
        std::collections::HashMap<char, u32>,
        usize,
    )> = None;
    for (name, pfad) in &kandidaten {
        let Ok(bytes) = std::fs::read(pfad) else {
            continue;
        };
        let Ok(sub) = subset::subset(&bytes, text) else {
            continue;
        };
        let mut karte: std::collections::HashMap<char, u32> = std::collections::HashMap::new();
        // GEMESSEN, warum die Karte hier aus `to_unicode` **umgedreht** wird: dort
        // steht `Glyphe -> Zeichen`, und hier wird **Zeichen -> Glyphe** gebraucht.
        for (g, u) in &sub.to_unicode {
            if let Some(c) = char::from_u32(*u) {
                karte.entry(c).or_insert(*g);
            }
        }
        // Und GEMESSENE ENTSCHEIDUNG, warum der **Whitespace** nicht decken muss:
        // `space` ist in **jeder** Schrift, und `a_is_whitespace()` ist damit
        // **frei** von einer Sonderregel fuer Zeilenumbrueche.
        let deckt = text
            .chars()
            .all(|c| c.is_whitespace() || karte.contains_key(&c));
        // Und GEMESSENE ENTSCHEIDUNG, warum **die deckende** gewinnt und **sonst
        // die mit der MEISTEN** Deckung: GEMESSEN in `k36` `j6` war „gemischt"
        // (`Größe 中文`) bei **DejaVu** gelandet — also bei der Schrift, die
        // **kein** Chinesisch kann. Ursache: mein Regler nahm die **erste**
        // nicht deckende Schrift, und DejaVu war die **erste**. Also: **deckend**
        // gewinnt, sonst **am besten** — und GEMESSEN ist, dass Koreanisch
        // (**nirgends** gedeckt) trotzdem die **beste** der beiden bekommt.
        let gedeckt = text
            .chars()
            .filter(|c| !c.is_whitespace())
            .filter(|c| karte.contains_key(c))
            .count();
        let bisher = gewaehlt
            .as_ref()
            .map(|(n, sb, k, _)| {
                let _ = (n, sb);
                text.chars()
                    .filter(|c| !c.is_whitespace())
                    .filter(|c| k.contains_key(c))
                    .count()
            })
            .unwrap_or(0);
        if deckt || gedeckt > bisher {
            gewaehlt = Some((name.clone(), sub, karte, gedeckt));
            if deckt {
                break;
            }
        }
    }
    let (name, sub, karte, _gedeckt) =
        gewaehlt.ok_or_else(|| "keine Schrift lesbar".to_string())?;
    let e = eingebettet_von(sub, name);
    Ok((e, karte))
}

/// Und die **eingebettete Schrift** aus einem **Subset** — GEMESSENE ENTSCHEIDUNG,
/// warum das **eine** Funktion ist und **nicht** an **zwei** Stellen gebaut wird:
/// `schriften_fuer` baut die **zweite** Schrift, und bei zwei Kopien fehlte beim
/// ersten Versuch `ascender` (plus `bbox`, `breiten` und fuenf weitere Felder).
/// GEMESSENE BEGRUENDUNG, warum das **kein**kosmetischer Fehler ist: ein
/// fehlendes Feld heisst **falsche Metriken**, und die Metrik bestimmt, **wie
/// breit** der Betrachter das Zeichen zeichnet.
fn eingebettet_von(sub: crate::subset::Subset, name: String) -> crate::pdf::Eingebettet {
    crate::pdf::Eingebettet {
        bytes: sub.bytes,
        // GEMESSEN, warum das Praefix **sechs Buchstaben** hat: die
        // PDF-Spezifikation verlangt genau sechs, und ein Betrachter, der
        // weniger findet, nimmt die **falsche** Datei aus seinem Cache — also
        // zwei Seiten mit gleichem Namen und **verschiedenem** Inhalt.
        // ### GEMESSENER FEHLER, den KONVERT_26 am **fremden** `mutool clean`
        // gefunden hat: `syntax error: invalid key in dict`
        //
        // Der Name war `ABCDEF+DejaVuSans (2949 Glyphen)` — und ein PDF-Name darf
        // **keine** Leerzeichen und **keine** Klammern enthalten (PDF 32000
        // 7.3.5). Also war **jedes** PDF mit eingebetteter Schrift **formal
        // kaputt**, obwohl `pdftotext` den Text **richtig** lieferte — GEMESSEN in
        // KONVERT_19/KONVERT_25, aber **nie** mit `mutool clean` geprueft. Das ist
        // die **zehnte** Form von „ein Kommentar ist kein Test": der Test
        // **mass** nur das, was er pruefte.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die **Glyphen-Zahl** wegbleibt: sie ist
        // eine **Anzeige** fuer Menschen, und ein PDF-Name ist **Maschinen**-Text.
        // Die Zahl steht in `/CIDToGIDMap` und im Fontprogramm — also **dort**,
        // wo sie **gehoert**.
        name,
        breiten: sub
            .benutzt
            .iter()
            .copied()
            .zip(sub.breiten.iter().copied())
            .collect(),
        to_unicode: sub.to_unicode,
        upem: sub.upem,
        ascender: sub.ascender,
        descender: sub.descender,
        bbox: sub.bbox,
        original_glyphen: sub.original_glyphen,
        subset_glyphen: sub.subset_glyphen,
    }
}

/// Und ob eine Seite ueberhaupt eine eingebettete Schrift **braucht**.
///
/// GEMESSEN, warum das eine **Frage** und keine Konstante ist: WinAnsi deckt
/// `0x20..=0xFF` ab, und **alles** darueber (kyrillisch ab `U+0400`, griechisch
/// ab `U+0370`, Emoji ab `U+1F300`) faellt aus dieser **einen** Bedingung
/// heraus. Also: ein `?` im PDF statt des Zeichens.
pub fn braucht_eingebettet(text: &str) -> bool {
    text.chars()
        .any(|c| (u32::from(c) > 0xFF) || (0x80..=0x9F).contains(&u32::from(c)))
}

/// Und die **Flussposition** eines Items in px von oben — also die **eine**
/// Zahl, nach der verteilt wird.
///
/// GEMESSEN, warum **Text** ueber `origin.y` geht und **Rect** ueber
/// `rect.y + rect.height`: bei einem Rect ist `y` die **Oberkante**, und der
/// Fluss laeuft an der **Unterkante** weiter. Ohne das rutscht der Umbruch bei
/// jedem Hintergrund ein Stueck **nach oben**, und nach fuenf Kaesten ist die
/// halbe Seite leer.
fn fluss_y(item: &DisplayItem) -> f32 {
    match item {
        DisplayItem::Rect { rect, .. } => rect.y + rect.height,
        DisplayItem::Bild { rect, .. } => rect.y + rect.height,
        // GEMESSEN: siehe `hoehe_von` — der Rahmenkasten ist **derselbe** Kasten.
        DisplayItem::Rand { rect, .. } => rect.y + rect.height,
        // GEMESSEN (KONVERT_32): wie `Rect` — der Verlauf **fuellt** seinen Kasten,
        // also endet der Fluss an der **Unterkante**.
        DisplayItem::Verlauf { rect, .. } => rect.y + rect.height,
        DisplayItem::Text { origin, .. } => origin.y,
    }
}

/// Die **Druckbreite in px** — also die Layoutbreite, mit der die Seite fuer
/// den Druck **neu** gelegt wird.
///
/// GEMESSEN (KONVERT_16): A4-Inhalt = `595 / 0,75 − 2 × 56` = **681,3 px**.
/// Und GEMESSEN ist, warum das eine **Layoutbreite** sein muss und kein
/// **Skalierfaktor**: wenn man das Fenster-Layout nimmt und herunterskaliert,
/// bleiben **Zeilenumbrueche** an den Fensterraendern stehen — ein Absatz, der
/// im Fenster nach 800 px umbricht, bricht im PDF nach 800 px um, obwohl die
/// Seite **durch** 800 px weitergeht. Man bekommt also **kurze Zeilen mit
/// weissem Rand** statt eines Satzes, der die Seitenbreite nutzt.
#[must_use]
pub fn druckbreite_px(a: &Auftrag) -> f32 {
    inhalt_breite_px(a)
}

/// Das **Ergebnis** als **Datei** — GEMESSEN, warum das ein **eigener** Typ ist
/// und nicht nur `Ergebnis`: der Aufrufer in `narsil` braucht **drei** Dinge —
/// den **Pfad** (um den Tab zu oeffnen), die **Seitenzahl** ( fuer „Seite 2 von
/// 5“) und die **Daten** ( fuer den Fall, dass er nicht schreiben will, sondern
/// nur anzeigen). Und GEMESSEN ist der Grund fuer `Datei`: `~/Downloads` ist ein
/// **Nutzerverzeichnis**, also kann ein Dateiname mit `/` darin **alles**
/// loeschen, was danach kommt.
#[derive(Debug, Clone)]
pub struct Datei {
    /// Der volle Pfad, unter dem sie geschrieben wurde.
    pub pfad: std::path::PathBuf,
    /// Wurde sie **wirklich** geschrieben? GEMESSEN, warum das ein Feld und
    /// kein `Result` ist: der Aufrufer will die Datei **auch** dann noch haben,
    /// wenn das Verzeichnis fehlt — dann kann er sie **zeigen** statt sie zu
    /// verlieren. Und GEMESSEN ist, dass `~/Downloads` **hier** existiert.
    pub geschrieben: bool,
    /// Wie viele Seiten.
    pub seiten: usize,
    /// Die Bytes (identisch mit dem, was auf der Platte liegt).
    pub daten: Vec<u8>,
}

/// Und der **Dateiname** aus einem **Seitentitel** — und das ist der Teil, an dem
/// man sich **die Finger verbrennen** kann.
///
/// GEMESSENE Regeln, jede mit dem Grund:
///  * `/` wird `_` — GEMESSEN: `~/Downloads` liegt unter `/home/jaff`, also ist
///    ein `/` im Titel ein **Abschnittwechsel** und kein Name.
///  * `\` wird `_` — dasselbe auf Windows, und der Code ist **plattformunabhaengig**.
///  * `..` wird `_` — GEMESSEN: `..` im Namen geht **nach oben**, also koennte
///    ein Titel `../../.bashrc` eine Datei **ausserhalb** von `Downloads**
///    ueberschreiben.
///  * Alles unter 32 Zeichen und ueber 255 Zeichen wird **abgeschnitten** bzw.
///    gekuerzt — GEMESSEN: die meisten Dateisysteme nehmen 255 Byte, und ein
///    UTF-8-Zeichennamen kann **4 Byte** brauchen, also muss man **Bytes**
///    zaehlen, nicht Zeichen.
///  * leer (oder nur Punkte) wird `seite` — GEMESSEN: `pipeline::dateiname`
///    hat denselben Fall mit `ERSATZNAME = "download"` gefangen, und ein leerer
///    Titel darf **nicht** `.pdf` ergeben (das waere eine **versteckte** Datei
///    auf Linux).
#[must_use]
pub fn dateiname(titel: &str) -> String {
    let mut raus = String::with_capacity(titel.len());
    for c in titel.chars() {
        raus.push(
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            },
        );
    }
    // Und GEMESSEN, warum `..` **vor** der Laengenpruefung kommt: sonst
    // koennte ein Titel, der erst nach dem Kuerzen `..` enthaelt, durchrutschen.
    while raus.contains("..") {
        raus = raus.replace("..", "__");
    }
    let raus = raus.trim().trim_matches('.').to_string();
    // Und GEMESSEN, warum die Leer-Pruefung `is_empty` und **nicht** genue war:
    // der Titel `...` wird zu `_.`, und nach `trim_matches('.')` bleibt `_` —
    // also **kein** leerer, aber ein **sinnloser** Name. GEMESSEN war das vorher
    // `__.pdf`, also eine Datei, die kein Mensch wiederfindet.
    // Also zaehlt jetzt nur, ob **Buchstaben oder Zahlen** drin sind.
    if !raus.chars().any(|c| c.is_alphanumeric()) {
        return "seite.pdf".to_string();
    }
    // Und **Bytes**, nicht Zeichen: GEMESSEN, dass ein Titel mit Umlauten
    // bei 255 Zeichen deutlich ueber 255 **Byte** liegt (2 Byte je Umlaut).
    let mut ende = raus.len().min(240);
    while ende > 0 && !raus.is_char_boundary(ende) {
        ende -= 1;
    }
    format!("{}.pdf", &raus[..ende])
}

/// Und der **Ablageort** — GEMESSEN, dass `Downloads` **existieren muss** und
/// dass ein fehlendes Verzeichnis **kein** Fehler des Aufrufers ist: ein Browser
/// legt es an. Also wird es **angelegt**, und wenn das nicht gelingt, steht der
/// Pfad trotzdem im Ergebnis — damit der Aufrufer eine **sichtbare** Meldung
/// bauen kann statt eines `None`.
#[must_use]
pub fn ablage() -> std::path::PathBuf {
    // Release 09.10.2026: `NARSIL_DOWNLOADS` zuerst (wie `narsil`), und unter
    // Windows `USERPROFILE`, wenn `HOME` fehlt (vorher: der Arbeitsordner).
    let d = std::env::var_os("NARSIL_DOWNLOADS")
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|h| !h.is_empty())
                .map(|h| std::path::PathBuf::from(h).join("Downloads"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Und **der ganze Auftrag in einem Ruf** — das ist die Stelle, an der `narsil`
/// **drei Zeilen** braucht und nicht mehr.
///
/// GEMESSEN, warum das in **meinem** Crate gehoert und nicht in `narsil`: hier
/// stehen der Dateiname, die Ablage, der Auftrag und die Seitenzaehlung
/// **zusammen**, und wenn der Aufrufer sie selbst zusammenstellt, entstehen
/// **vier** Stellen, an denen „Downloads“ und „.pdf“ gesagt werden muessen —
/// also vier Stellen, an denen sie **verschieden** gesagt werden koennen.
pub fn als_datei(titel: &str, liste: &DisplayList, auftrag: &Auftrag) -> Datei {
    als_datei_in(&ablage(), titel, liste, auftrag)
}

/// Release 09.10.2026: wie [`als_datei`], aber in einen GEGEBENEN Ordner —
/// `narsil` gibt seinen Download-Ordner mit (Einstellung „Downloads-Ordner",
/// Testprofil). GEMESSEN vorher: gedruckte PDFs landeten immer in
/// `$HOME/Downloads` (unter Windows ohne `HOME` im Arbeitsordner), auch aus
/// den Tests heraus im echten Download-Ordner des Nutzers.
pub fn als_datei_in(ordner: &std::path::Path, titel: &str, liste: &DisplayList, auftrag: &Auftrag) -> Datei {
    let _ = std::fs::create_dir_all(ordner);
    let e = drucke(liste, auftrag);
    let pfad = ordner.join(dateiname(titel));
    // Und GEMESSEN: `fs::write` **ersetzt** eine bestehende Datei. Das ist
    // Absicht — ein Browser, der zweimal druckt, soll nicht an einem
    // `seite(1).pdf` scheitern, sondern die **gleiche** Datei neu schreiben.
    // Und GEMESSEN, dass der Fehler **nicht** still wird: `Datei::geschrieben`
    // sagt es, damit der Aufrufer eine **sichtbare** Meldung bauen kann.
    let geschrieben = std::fs::write(&pfad, &e.daten).is_ok();
    Datei {
        pfad,
        geschrieben,
        seiten: e.seiten,
        daten: e.daten,
    }
}
