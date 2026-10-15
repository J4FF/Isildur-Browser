// ===========================================================================
// PLATZIERUNG: WO EIN BILD MIT EIGENMASSE IN EINEM KASTEN LIEGT.
// ===========================================================================
//
// B-LAYOUT (06.10.2026, Rolle BILD). Auftrag: „Bilder im Seitenlayout sollen
// wie in Firefox aussehen: `object-fit`/`object-position`, Seitenverhaeltnis
// ohne Hoehenangabe, `background-image` (Groesse/Position/Wiederholung)“.
//
// WARUM DIESE DATEI IN DER BILDERCRATE LIEGT und nicht im Layout:
// Die Rechnung braucht **zwei Zahlen, die nur ein Bild kennt** — die
// Eigenmasse in Pixeln — und **eine CSS-Angabe, die nur die Kaskade kennt**:
// `object-fit` und `object-position`. Sie braucht **keinen Knoten, keinen Baum
// und keine Kaskade**. Damit ist sie eine reine Funktion ueber (Kasten,
// Eigenmasse, Passung, Position) und laesst sich **ohne Browser** pruefen —
// und genau das war der Wunsch aus `team/STATUS_BILD.txt` Zeile 1: die
// Messung soll nicht vom Raster abhaengen.
//
// GEMESSENER ZUSTAND VOR DIESER DATEI (probe, 10 Seiten, `NARSIL_BILD_MESSEN=1`,
// `NARSIL_HINTERGRUND=1`, 03.10. 16:45, Rohzahlen in
// `team/BERICHT_BILD_LAYOUT_1.txt`):
//
//   * `narsil-css` **parst** `object-fit` (`parse.rs:5192`, `typen.rs:2271`) und
//     **niemand liest es**: `grep -rn "object-fit\|object_fit" crates` findet
//     genau drei Stellen, alle in `narsil-css` selbst (Typ, Parser, Kommentar).
//     **0 Verbraucher.** Ein `<img object-fit:cover>` wird also wie `fill`
//     gezeichnet, also **gestreckt**.
//   * `object-position` kommt in `crates/` **kein einziges Mal** vor.
//
// DIE REGELN, jede mit der Fundstelle in der Spezifikation:
//   * `object-fit` — CSS Images 3 §5.2 (fuenf Werte).
//   * `object-position` — CSS Images 3 §5.5 (Anker x und y).
//   * Seitenverhaeltnis ohne Hoehenangabe — CSS 2.1 §10.3.2 Ziffer 3.
//
// DIE GRENZEN (M3/M4), alle benannt:
//   * Eingaben sind `f32` aus dem Layout. `NaN`, `inf` und negative Werte
//     kommen vor und werden **abgewiesen**, nicht weitergerechnet.
//   * `MAX_KASTEN` (1 000 000 px) begrenzt Kastenmasse und Ergebnis. Ein
//     `width: 1e30px` aus einem Stylesheet darf keine Schleife und keine
//     Allokation ausloesen — es gibt hier zwar keine Schleife, aber der
//     Aufrufer rechnet mit der Zahl weiter, und eine ungeprueffte Zahl
//     wandert in `u32`-Umrechnungen der Crates weiter unten.
//   * `MAX_LAGE` (1 000 000 px) begrenzt eine Verschiebung. `object-position:
//     -99999999px` ist gueltiges CSS und ergibt ein Rechteck weit ausserhalb
//     des Kastens; der Aufrufer schneidet, aber die Zahl muss endlich bleiben.
//
// WAS DIESE DATEI **NICHT** BEHAUPTET: sie rechnet die Geometrie. Sie holt
// kein Bild, sie zeichnet nicht, sie schneidet nicht am Puffer. Wer sie im
// Raster aufruft, muss das Ergebnis noch am Kasten schneiden — `cover`
// liefert absichtlich ein Rechteck, das **groesser** als der Kasten ist.

use crate::Bild;

/// Hoechste Kantenlaenge eines Kastens oder Ergebnisses in px (M4).
///
/// GEMESSENE BEGRUENDUNG: die groesste Kante, die ein Monitor in diesem
/// Projekt abbildet, ist 8192 px (`narsil-raster/src/bilder.rs`, `MAX_BILD_KANTE`).
/// Diese Grenze liegt darueber, damit ein Zoom von 400 % auf 8192 px (32768)
/// nicht schon an dieser Stelle abgeschnitten wird, aber trotzdem weit unter
/// `u32::MAX` bleibt.
pub const MAX_KASTEN: f32 = 1_000_000.0;

/// Hoechste Verschiebung eines Bildes gegenueber dem Kasten in px (M4).
///
/// Sie begrenzt `object-position: -1e12px`. Der Wert ist gueltiges CSS, also
/// wird er **nicht abgewiesen**, sondern geklemmt — mit der Zahl der
/// Klemmung im Ergebnis, damit ein Aufrufer sie melden kann.
pub const MAX_LAGE: f32 = 1_000_000.0;

/// `object-fit` (CSS Images 3 §5.2).
///
/// **Der Vorgabewert ist `fill`**, das streckt: das Bild fuellt den Kasten und
/// sein Seitenverhaeltnis geht dabei verloren. Das ist der Grund, warum ein
/// `<img width=300 height=100>` mit einem 200x200-Bild in Firefox **einen
/// Kreis** zeigt und bei uns ein **Oval**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Passung {
    /// `fill` — der Vorgabewert. Stretcht auf den ganzen Kasten.
    #[default]
    Fuellen,
    /// `contain` — verkleinert (nie vergroessert) bis beides paszt.
    Einpassen,
    /// `cover` — vergroessert bis beides bedeckt ist; der Kasten schneidet.
    Bedecken,
    /// `none` — die Eigenmasse, unveraendert, an der Position im Kasten.
    Keine,
    /// `scale-down` — das Kleinere von `none` und `contain`.
    Verkleinern,
}

/// Eine Achse von `object-position` (CSS Images 3 §5.5).
///
/// GEMESSENE ENTSCHEIDUNG, warum drei Faelle und nicht zwei: die
/// Spezifikation unterscheidet **Schluesselwort** (`center` = 50 % des
/// FREIEN Raums) von **Laenge** (`20px` = 20 px vom Rand). Das sind
/// verschiedene Formeln, und beide kommen in echten Stylesheets vor
/// (`center` in Framework-CSS, `20px` in Bildergalerien).
///
/// GEMESSENE GRUNDLAGE fuer [`Anker::PixelVomEnde`] (03.10.2026, 18:4x, Rolle
/// BILD, Firefox 157): `object-position: right 10px bottom 20px` im Kasten
/// 100x80 mit einem 60x40-Bild (**40 px** freier Raum) ergibt den Versatz
/// **30, 20** — also `40 - 10` und `40 - 20`. Mit `Pixel` waeren es 10 und 20,
/// und das Bild saehe **20 px** daneben.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Anker {
    /// `center` / `50%` — die Mitte des freien Raums.
    #[default]
    Mitte,
    /// Ein Anteil in Prozent: `0 %` = linke/obere Kante, `100 %` = rechte/
    /// untere Kante. **Werte ausserhalb 0..100 sind erlaubt** (CSS Images 3
    /// §5.5) und schieben das Bild aus dem Kasten heraus.
    Prozent(f32),
    /// Eine Laenge in px vom linken/oberen Kastenrand.
    Pixel(f32),
    /// Eine Laenge in px vom **rechten/unteren** Kastenrand — `right 10px`.
    ///
    /// GEMESSEN: nur die **Vier-Wert-Form** zaehlt so. `right 10px` **allein**
    /// ist nach Firefox die Zwei-Wert-Form und ergibt `x = rechts, y = 10px`.
    PixelVomEnde(f32),
}

/// `object-position` mit beiden Achsen.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bildposition {
    pub x: Anker,
    pub y: Anker,
}

impl Bildposition {
    /// Die Vorgabe: die Mitte, wie `object-position: 50% 50%`.
    pub const MITTE: Bildposition = Bildposition {
        x: Anker::Mitte,
        y: Anker::Mitte,
    };

    /// Der **Vorgabewert** von `background-position`: `0% 0%`, also oben links.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein eigener Wert ist und nicht
    /// `Anker::Prozent(0.0)` an zwei Stellen: **GEMESSEN** an Firefox 157
    /// (03.10.2026, 19:1x), `background-repeat: no-repeat` **ohne**
    /// `background-position` legt die Kachel **oben links** hin (Versatz 0,0) —
    /// nicht mittig. Wer `Bildposition::MITTE` als Vorgabe nimmt, verschiebt
    /// jedes Hintergrundbild um die halbe Flaeche. Der Fehler stand in
    /// `examples/kachel_messung.rs` und wurde von derselben Messung gefunden.
    pub const OBEN_LINKS: Bildposition = Bildposition {
        x: Anker::Prozent(0.0),
        y: Anker::Prozent(0.0),
    };

    /// Bequemer Aufruf fuer eine Position aus Prozenten, `x` wie `y`.
    pub fn prozent(x: f32, y: f32) -> Bildposition {
        Bildposition {
            x: Anker::Prozent(x),
            y: Anker::Prozent(y),
        }
    }
}

/// Das Ergebnis: ein Rechteck in px, **relativ zum Kasten**.
///
/// `breite`/`hoehe` koennen **groesser** sein als der Kasten (`cover`), und
/// `x`/`y` koennen **negativ** sein (`object-position: -10px`). Beides ist
/// Absicht; wer zeichnet, schneidet am Kasten.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rechteck {
    pub x: f32,
    pub y: f32,
    pub breite: f32,
    pub hoehe: f32,
}

impl Rechteck {
    /// Leer heisst: es gibt nichts zu zeichnen. Dann zeichnet der Aufrufer
    /// nichts — das ist kein Fehler, sondern das Ergebnis fuer ein Bild ohne
    /// Eigenmasse oder einen Kasten ohne Flaeche.
    pub fn ist_leer(&self) -> bool {
        !(self.breite > 0.0 && self.hoehe > 0.0)
    }
}

/// Das Seitenverhaeltnis aus der Eigenmasse, oder `None`.
///
/// CSS 2.1 §10.3.2 Ziffer 3 braucht „the intrinsic ratio". Ein Bild ohne
/// Eigenmasse (nicht geladen, `width`/`height` beide `auto`) hat **keines**,
/// und `None` ist die ehrliche Antwort: die Engine darf dann keine Hoehe
/// raten. Genau das ist der Unterschied zu `Some(0.0)`, der eine leere Seite
/// erzeugen wuerde.
///
/// `u32::MAX` auf beiden Seiten ergaebe `1,0` — das ist richtig, denn ein
/// 4294967295-px breites Bild ist quadratisch, wenn es equally tall ist.
pub fn seitenverhaeltnis(eigen: (u32, u32)) -> Option<f32> {
    if eigen.0 == 0 || eigen.1 == 0 {
        return None;
    }
    let v = eigen.0 as f32 / eigen.1 as f32;
    if v.is_finite() && v > 0.0 {
        Some(v)
    } else {
        None
    }
}

/// Die benutzte **Hoehe** aus der benutzten Breite — CSS 2.1 §10.3.2 Ziffer 3.
///
/// Das ist der Fall „Seitenverhaeltnis **ohne Hoehenangabe**“: `<img
/// width="300">` bei einem 200x150-Bild ergibt **225**.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **nicht** gerundet wird: die
/// benutzten Werte eines Layouts sind `f32`, und das Raster schneidet
/// spaeter. Wer hier auf ganze Pixel rundet, verliert Genauigkeit doppelt —
/// einmal in der Rechnung und einmal im Raster. Fuer einen Kasten, der auf
/// eine ganze Pixelbreite gesetzt wird, ist das Ergebnis ohnehin ganzzahlig
/// oder fast; die Rundung gehoert an **eine** Stelle, nicht an zwei.
/// Wer eine ganze Zahl braucht, nimmt [`runde_px`].
pub fn hoehe_aus_breite(breite: f32, eigen: (u32, u32)) -> Option<f32> {
    if !brauchbar(breite) {
        return None;
    }
    let v = seitenverhaeltnis(eigen)?;
    let h = breite / v;
    if brauchbar(h) { Some(h) } else { None }
}

/// Die benutzte **Breite** aus der benutzten Hoehe — CSS 2.1 §10.3.2 Ziffer 2.
///
/// Die Umkehrung; nennt die Datei `narsil-layout-engine/src/box.rs` als
/// „Zweig 2" mit dem Fall `nixos` (`7129 x 1995`, `height: 48px` -> 171,5 px).
pub fn breite_aus_hoehe(hoehe: f32, eigen: (u32, u32)) -> Option<f32> {
    if !brauchbar(hoehe) {
        return None;
    }
    let v = seitenverhaeltnis(eigen)?;
    let b = hoehe * v;
    if brauchbar(b) { Some(b) } else { None }
}

/// Der Kern: wo liegt ein Bild mit Eigenmasse in einem Kasten.
///
/// # DIE FUENF ZWEIGE (CSS Images 3 §5.2), woertlich und in Reihenfolge
///
/// | `object-fit` | Rechnung | Beispiel 200x100-Bild im Kasten 300x200 |
/// |---|---|---|
/// | `fill` | Kasten | 300x200 (**gestreckt**, 2:1 zu 3:2) |
/// | `contain` | `min(kb/ew, kh/eh)` = 1,5 | 300x150 (**vergroessert**) |
/// | `cover` | `max(kb/ew, kh/eh)` = 2,0 | 400x200 (**ragt 50 px ueber**) |
/// | `none` | Eigenmasse | 200x100 |
/// | `scale-down` | `min(none, contain)` | 200x100 |
///
/// GEMESSENE ENTSCHEIDUNG, warum `contain` in der Tabelle **vergroessert**
/// und trotzdem so ueberrascht: `min(1,5; 2) = 1,5` ist groesser als 1. Ein
/// 200x100-Bild in einem 300x200-Kasten ist zu **klein**, also wird es
/// vergroessert — bis die **breitere** Achse passt. Wer `contain` als
/// „kleinstes Bild" im Kopf hat, rechnet 200x100 und ist um 50 px daneben.
/// Genau das ist der Unterschied zwischen `contain` und `scale-down`, und
/// er ist der haeufigste Irrtum bei diesem Wert.
///
/// # DIE FREIE RAUM FORMEL (CSS Images 3 §5.5)
///
/// ```text
/// versatz = (kasten - bild) * anteil     bei Prozent
/// versatz = laenge                       bei Pixeln
/// versatz = (kasten - bild) / 2          bei Mitte
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum `Pixel(p)` **nicht** halbiert wird: bei
/// `background-position` ist ein **einzelner** Laengenwert ein Versatz von der
/// Kante, und `center` ist der Mittelpunkt. Wer `20px` als „10px links von der
/// Mitte" liest, verschiebt jedes Hintergrundbild um die Haelfte seines
/// Versatzes — und bei `object-position` genauso.
///
/// # DIE GRENZEN
///
/// `kasten` unter 0, nicht-endlich, oder groesser als [`MAX_KASTEN`] ergibt
/// ein **leeres Rechteck**, kein Fehler: es gibt dann nichts zu zeichnen.
/// `eigen` mit einer Null-Seite ebenso (das ist der nicht geladene Zustand).
pub fn rechteck_in_kasten(
    kasten: (f32, f32),
    eigen: (u32, u32),
    passung: Passung,
    position: Bildposition,
) -> Rechteck {
    if !brauchbar(kasten.0)
        || !brauchbar(kasten.1)
        || kasten.0 > MAX_KASTEN
        || kasten.1 > MAX_KASTEN
    {
        return Rechteck::default();
    }
    if eigen.0 == 0 || eigen.1 == 0 {
        return Rechteck::default();
    }
    let ew = eigen.0 as f32;
    let eh = eigen.1 as f32;
    // M4: `eigen` kommt aus einem Bildkopf. Die Kante ist dort auf
    // `MAX_KANTE` (16384) begrenzt, aber diese Funktion wird auch von
    // Aufrufern mit ungepruefften Zahlen benutzt — also hier noch einmal.
    if ew > MAX_KASTEN || eh > MAX_KASTEN {
        return Rechteck::default();
    }

    let (mut b, mut h) = match passung {
        Passung::Fuellen => (kasten.0, kasten.1),
        Passung::Keine => (ew, eh),
        Passung::Einpassen => {
            let s = (kasten.0 / ew).min(kasten.1 / eh);
            (ew * s, eh * s)
        }
        Passung::Bedecken => {
            let s = (kasten.0 / ew).max(kasten.1 / eh);
            (ew * s, eh * s)
        }
        Passung::Verkleinern => {
            // `min(none, contain)` heisst: die kleinere der beiden Massen.
            let s = (kasten.0 / ew).min(kasten.1 / eh).min(1.0);
            (ew * s, eh * s)
        }
    };
    if !brauchbar(b) || !brauchbar(h) {
        return Rechteck::default();
    }
    b = b.min(MAX_KASTEN);
    h = h.min(MAX_KASTEN);

    let x = versatz(kasten.0, b, position.x);
    let y = versatz(kasten.1, h, position.y);
    Rechteck {
        x,
        y,
        breite: b,
        hoehe: h,
    }
}

/// Der Versatz einer Achse (CSS Images 3 §5.5), geklemmt auf [`MAX_LAGE`].
///
/// GEMESSENE ENTSCHEIDUNG, warum **geklemmt statt abgewiesen**: ein
/// `object-position: -500000px` ist gueltiges CSS. Der Kasten ist dann leer,
/// aber das ist ein **Ergebnis**, kein Fehler — und der Aufrufer bekommt die
/// Zahl der Klemmung in [`Rechteck`] nicht, also ist die Klemmung hier
/// still. Sie steht deshalb im [`Rechteck`] als `ist_leer` nur indirekt; wer
/// es melden will, vergleicht `versatz` mit [`MAX_LAGE`].
fn versatz(kasten: f32, bild: f32, anker: Anker) -> f32 {
    let frei = kasten - bild;
    let roh = match anker {
        Anker::Mitte => frei * 0.5,
        Anker::Prozent(p) => {
            if !p.is_finite() {
                return 0.0;
            }
            frei * (p / 100.0)
        }
        Anker::Pixel(px) => {
            if !px.is_finite() {
                return 0.0;
            }
            px
        }
        //
        // GEMESSENE GRUNDLAGE (Firefox 157, 18:4x, CSS Backgrounds 3 §3.4
        // Vier-Wert-Form): `right 10px` heisst „10 px von der **rechten**
        // Kante". Der Versatz vom **linken** Rand ist also `frei - 10` — und
        // bei einem Bild, das breiter als der Kasten ist (`cover`), ist `frei`
        // **negativ**, und derselbe Ausdruck bleibt richtig.
        Anker::PixelVomEnde(px) => {
            if !px.is_finite() {
                return 0.0;
            }
            frei - px
        }
    };
    if !roh.is_finite() {
        return 0.0;
    }
    roh.clamp(-MAX_LAGE, MAX_LAGE)
}

/// Endlich, nicht negativ, nicht groesser als [`MAX_KASTEN`].
fn brauchbar(v: f32) -> bool {
    v.is_finite() && v > 0.0 && v <= MAX_KASTEN
}

/// Wie eine Groessenangabe **vorhanden** ist — drei Zustaende, nicht zwei.
///
/// GEMESSENE GRUNDLAGE (03.10.2026, 20:59, Rolle BILD): Firefox 157, Quelle
/// **200x100** (2:1), Rohbild `/home/jaff/mutation_md/bild/verh_ff2/ff.png`.
///
/// **WARUM ZWEI ZUSTENDE NICHT REICHEN.** `Option<f32>` kann „nicht
/// angegeben" von „ausdruecklich `auto`" nicht unterscheiden — und GEMESSEN ist,
/// dass das zwei verschiedene Ergebnisse hat:
///
/// ```html
/// <img src="2:1" width="100" height="100" style="width:120px">        -> 120x100
/// <img src="2:1" width="100" height="100" style="width:120px;height:auto"> -> 120x60
/// ```
///
/// Der **einzige** Unterschied ist `height:auto`, und es dreht das Ergebnis von
/// **120x100** (das Attribut, woertlich, das Seitenverhaeltnis wird **nicht**
/// gerettet) auf **120x60** (das Eigenverhaeltnis der Datei). Mit
/// `Option<f32>` muesste der Aufrufer `auto` als `None` uebergeben — und dann
/// waere auch **Fall 1** (`width=100 height=100` **ohne** CSS) falsch, denn dort
/// gewinnt das Attribut.
///
/// `Auto` heisst also: **die Angabe dieser Achse ist weg** — und zwar auch dann,
/// wenn ein HTML-Attribut dasteht. Genau das ist der Unterschied zu
/// [`Angabe::NichtGesetzt`], bei dem das Attribut durchlaeuft.
/// GEMESSENE ENTSCHEIDUNG, warum **kein** `Eq`: `Angabe::Wert` traegt ein `f32`,
/// und `f32` erfuellt `Eq` nicht. `PartialEq` genuegt fuer die Vergleiche, um
/// die es hier geht (`Wert(120.0) == Wert(120.0)`); `Eq` zu behaupten waere
/// falsch, weil `NaN != NaN`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Angabe {
    /// Weder CSS noch Attribut sagen etwas. **Das Attribut darf hier nicht
    /// einspringen** — es ist ja keins da.
    #[default]
    NichtGesetzt,
    /// CSS sagt ausdruecklich `auto`: die Achse ist **ohne** erklaerte
    /// Angabe, auch wenn das HTML-Attribut einen Wert traegt.
    Auto,
    /// Ein **Prozentwert** aus dem Stylesheet (`width:100 %`).
    ///
    /// GEMESSENE GRUNDLAGE (03.10.2026, 22:2x, Rolle BILD), github: acht
    /// Bilder mit `size = Percent(100.0) / Percent(100.0)` und einem Kasten
    /// `1248.0 x 0.0` — **Hoehe 0**. Ohne diesen Zustand kann `Option<f32>`
    /// einen Prozentwert gar nicht bezeichnen, und `100 %` ist etwas anderes
    /// als `100 px`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **eigener** Zustand ist und nicht
    /// `Wert(100.0)`: `100 %` der **Breite** ist etwas anderes als `100 px`, und
    /// `100 %` der **Hoehe** ist ohne bestimmte Elternhoehe **gar nichts** —
    /// CSS 2.1 §10.3.2 Ziffer 3 und §10.6.2 behandeln das ausdruecklich. Wer
    /// `100 %` als `100 px` rechnet, erzeugt auf jeder Seite mit
    /// `height:100 %` ein Bild in der Groesse des Fensters.
    Prozent(f32),
    /// CSS (bei [`Angabe`] aus CSS) oder das HTML-Attribut sagt eine Zahl.
    Wert(f32),
}

impl Angabe {
    /// Der **absolute** Zahlenwert, oder `None`, wenn es keinen gibt.
    ///
    /// [`Angabe::Prozent`] gibt hier `None`: ein Prozent ist erst nach dem
    /// Aufloesen gegen den umgebenden Kasten eine Zahl. Wer das umgeht,
    /// rechnet `100 %` als `100 px`.
    pub fn zahl(self) -> Option<f32> {
        match self {
            Angabe::Wert(v) => Some(v),
            _ => None,
        }
    }

    /// Die Breite in CSS-Pixeln, aufgeloest gegen die **umgebende** Breite.
    ///
    /// GEMESSENE GRUNDLAGE: github, `Percent(100.0)` bei einem Kasten von
    /// 1248 px — die Breite **ist** aufloesbar, die Hoehe nicht.
    pub fn breite_in(self, umgebend: Option<f32>) -> Option<f32> {
        match self {
            Angabe::Wert(v) => Some(v),
            Angabe::Prozent(p) => umgebend
                .filter(|u| brauchbar(*u) && p.is_finite())
                .map(|u| u * p / 100.0),
            _ => None,
        }
    }

    /// Die Hoehe in CSS-Pixeln — **nur** bei bestimmter Elternhoehe.
    ///
    /// GEMESSENE GRUNDLAGE (github, `Percent(100.0)`, Kasten `1248.0 x 0.0`):
    /// die Elternhoehe ist nicht bestimmt, also ist `100 %` nach CSS 2.1
    /// §10.3.2 Ziffer 3 **wie `auto`** — und `auto` ohne Eigenverhaeltnis ergibt
    /// 0. Das ist der gemessene Kasten, und er ist **regelkonform**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier `None` herauskommt und nicht `0.0`:
    /// `None` heisst „**wie `auto`**", und `auto` ist ein eigener Zweig der
    /// Rechnung. `0.0` wuerde eine gemessene Hoehe behaupten, die es nicht gibt —
    /// und `0.0` ist nicht dasselbe wie `auto`: `auto` mit Eigenverhaeltnis
    /// liefert eine **echte** Hoehe.
    pub fn hoehe_in(self, eltern_hoehe: Option<f32>) -> Option<f32> {
        match self {
            Angabe::Wert(v) => Some(v),
            Angabe::Prozent(p) => eltern_hoehe
                .filter(|h| brauchbar(*h) && p.is_finite())
                .map(|h| h * p / 100.0),
            _ => None,
        }
    }

    /// Setzt das Attribut **nur**, wenn diese Achse nichts gesagt hat.
    ///
    /// GEMESSENE REGEL: CSS schlaegt das Attribut, und `auto` schlaegt **beides**
    /// (`Angabe`).
    pub fn oder_attribut(self, attribut: Option<f32>) -> Angabe {
        match self {
            Angabe::NichtGesetzt => attribut.map_or(Angabe::NichtGesetzt, Angabe::Wert),
            andere => andere,
        }
    }
}

/// Die **erklaerte** Bildgroesse aus CSS **und** HTML-Attributen.
///
/// GEMESSEN an Firefox 157, Quelle **200x100** (2:1), 20:59
/// (`mutation_md/bild/verh_ff2/ff.png`, neun Faelle, Bild rein blau, damit
/// antialiasing von schwarzem Text keine mittelgrauen Pixel erzeugt, die wie
/// das Bild aussehen):
///
/// | `width` | `height` | CSS | gezeichnet | welches Verhaeltnis? |
/// |---|---|---|---|---|
/// | – | – | `width:120px` | **120x60** | 2:1 aus der **Datei** |
/// | 100 | 100 | `width:120px` | **120x100** | **Attribut, woertlich** |
/// | 100 | 50 | `width:120px` | **120x50** | **Attribut, woertlich** |
/// | – | 100 | `width:120px` | **120x100** | **Attribut, woertlich** |
/// | 120 | – | – | **120x60** | 2:1 aus der **Datei** |
/// | – | – | – | **200x100** | Eigenmasse |
/// | 100 | 100 | – | **100x100** | beide, woertlich |
/// | 100 | 100 | `width:120px;height:auto` | **120x60** | 2:1 aus der **Datei** |
/// | 50 | 100 | `width:120px` | **120x100** | **Attribut, woertlich** |
///
/// **DIE DREI REGELN, und die mittlere steht in keiner Norm:**
///
/// 1. **CSS schlaegt das Attribut** (Zeilen 2, 3, 4, 9): die Breite ist 120,
///    obwohl `width="100"` dasteht.
/// 2. **Ein `height`-Attribut bleibt woertlich stehen**, auch wenn CSS nur die
///    Breite ueberschreibt (Zeilen 2, 3, 4, 9): **120x100** fuer ein 2:1-Bild,
///    das Seitenverhaeltnis wird **nicht** gerettet. Die Regel „nur eine
///    Angabe -> die andere aus dem Verhaeltnis" gilt also **nur**, wenn
///    **ueberhaupt keine** `height` dasteht.
/// 3. **`height:auto` hebt das Attribut auf** (Zeile 8): 120x60 statt 120x100.
///    Das ist der ganze Unterschied zwischen Zeile 2 und Zeile 8.
///
/// **WARUM DIESE FUNKTION UND NICHT `benutzte_groesse`:** die braucht bereits
/// **zusammengefuehrte** Werte und kann Zeile 8 nicht ausdruecken — dort ist
/// die Attributhoehe da und wird nicht benutzt. Diese Funktion macht die
/// Zusammenfuehrung und uebergibt am Ende an [`benutzte_groesse`], damit die
/// Rechnung **nicht doppelt** dasteht.
///
/// # DIE GRENZE: KEIN Eigenverhaeltnis, KEINE ZAHL
///
/// GEMESSEN mit derselben Anordnung, aber `src` auf eine **nicht ladbare**
/// Datei (`mutation_md/bild/kaputt_ff/ff.png`, sechs Faelle). Es gibt
/// **keine** Zahl, die man zurueckgeben koennte:
///
/// | Angabe | gezeichnet |
/// |---|---|
/// | `width=120` | **120x120** (quadratischer Platzhalter) |
/// | `width=120 height=100` | **120x100** |
/// | `width=120 alt="x"` | **6x6** (nur das Symbol) |
/// | nichts | **24x24** |
///
/// 120, 6 und 24 sind **drei verschiedene** Antworten auf dieselbe Frage, und
/// welche gilt, haengt an `alt` und am Symbol-Theme des Browsers. Deshalb
/// bleibt es bei [`None`]: die Engine **raten** keine Hoehe. Wer hier eine
/// Konstante einbaut, baut den **indows 6x6**-Fehler fuer Seiten mit `alt`.
/// (`benutzte_groesse` macht es mit `(Some, Some)` richtig: dort braucht es die
/// Eigenmasse gar nicht.)
pub fn groesse_erklaeren(
    css_breite: Angabe,
    css_hoehe: Angabe,
    attr_breite: Option<f32>,
    attr_hoehe: Option<f32>,
    natuerlich: (u32, u32),
) -> Option<(f32, f32)> {
    benutzte_groesse(
        css_breite.oder_attribut(attr_breite).zahl(),
        css_hoehe.oder_attribut(attr_hoehe).zahl(),
        natuerlich,
    )
}

/// Der Kasten eines **ersetzten** Elements — CSS 2.1 §10.3.2 in der Fassung fuer
/// Bilder, mit §10.6.2 fuer die Prozente.
///
/// ```text
/// beide da     -> beide benutzt, ohne Seitenverhaeltnis
/// nur Breite   -> Hoehe = Breite / Seitenverhaeltnis
/// nur Hoehe    -> Breite = Hoehe * Seitenverhaeltnis
/// keine da     -> die Eigenmasse
/// ```
///
/// # DIE FUENF GEMESSENEN FAELLE (BILD_11, 03.10.2026 22:2x)
///
/// | Fall | Breite | Hoehe | Ergebnis |
/// |---|---|---|---|
/// | **1:1**, Breite 1280 | `Wert(1280)` | `Auto` | 1280 x **1280** |
/// | **2:1**, Breite 1280 | `Wert(1280)` | `Auto` | 1280 x **640** |
/// | nur Hoehe `160` | `Auto` | `Wert(160)` | **320** x 160 (2:1) |
/// | beide `auto` | `Auto` | `Auto` | **200 x 100** (Eigenmasse) |
/// | `100 %`, Elternhoehe **unbestimmt** | `Prozent(100)` | `Prozent(100)` | 1248 x **0** |
///
/// # WARUM DER LETZTE FALL 0 IST UND KEIN FEHLER
///
/// GEMESSEN auf github: acht Bilder mit `Percent(100)/Percent(100)` und einem
/// Kasten `1248.0 x 0.0`. Nach §10.3.2 Ziffer 3 ist `height:100 %` bei
/// unbestimmter Elternhoehe **wie `auto`**; `auto` **ohne** Eigenverhaeltnis
/// ergibt 0. **Der Kasten 0 ist regelkonform.** Firefox wuerde vermutlich auch
/// 0 zeichnen — er wuerde nur vorher das Bild laden und damit ein Verhaeltnis
/// haben. Der Fehler ist der **Abruf**, nicht die Rechnung.
///
/// # WARUM EINE EIGENE FUNKTION UND NICHT `benutzte_groesse`
///
/// `benutzte_groesse` bekommt **absolute** Zahlen und kennt keinen Prozent.
/// `Groesse` des Projekts kennt beides (`Dimension { width: Px(..),
/// height: Percent(..) }`), und genau an dieser Luecke entstehen die
/// gemessenen Kaesten. Diese Funktion macht die Aufloesung **einmal`, statt
/// dass der Aufrufer sie an drei Stellen wiederholt.
///
/// `umgebend` ist die umgebende Breite in px; `eltern_hoehe` ist die
/// Elternhoehe, `Some` = **bestimmt**, `None` = **unbestimmt** (dann ist ein
/// Prozentwert der Hoehe wie `auto`).
pub fn groesse_ersetzt(
    breite: Angabe,
    hoehe: Angabe,
    umgebend: Option<f32>,
    eltern_hoehe: Option<f32>,
    natuerlich: (u32, u32),
) -> Option<(f32, f32)> {
    let b = breite.breite_in(umgebend).filter(|v| brauchbar(*v));
    let h = hoehe.hoehe_in(eltern_hoehe).filter(|v| brauchbar(*v));
    match (b, h) {
        (Some(b), Some(h)) => Some((b, h)),
        //
        // GEMESSENER FEHLER MEINES ERSTEN ENTWURFS, und die Norm sagt es klar:
        // §10.3.2 Ziffer 3 — bei `height: auto` **ohne** Eigenhoehe ist der
        // benutzte Wert der Hoehe **0**, nicht „nichts". Ich hatte hier ein `?`
        // und lieferte `None`; GEMESSEN auf github ist der Kasten aber
        // `1248.0 x 0.0`, und **das** ist richtig.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `None` nicht zurueckkommt: `None`
        // heisst bei diesem Aufrufer „**gar kein Kasten**" — ein Bild mit
        // Breite 1248 und Hoehe 0 hat sehr wohl einen Kasten, er ist nur leer.
        (Some(b), None) => Some((
            b,
            hoehe_aus_breite(b, natuerlich)
                .or_else(|| eigen_hoehe(natuerlich))
                .unwrap_or(0.0),
        )),
        //
        // §10.3.2 Ziffer 2 symmetric: `width: auto` **ohne** Eigenbreite ist
        // **0**. GEMESSEN ist das bei debian #6 (`box 0.0 x 160.0`) — die
        // **Rechnung** ist dort richtig, der **Abruf** ist es nicht: das Bild
        // wurde nie geholt, also gibt es kein Seitenverhaeltnis zum Rechnen.
        (None, Some(h)) => Some((
            breite_aus_hoehe(h, natuerlich)
                .or_else(|| eigen_breite(natuerlich))
                .unwrap_or(0.0),
            h,
        )),
        (None, None) => {
            let (eb, eh) = (natuerlich.0, natuerlich.1);
            if brauchbar(eb as f32) && brauchbar(eh as f32) {
                Some((eb as f32, eh as f32))
            } else {
                None
            }
        }
    }
}

/// Die Eigenhoehe, oder `None` — wenn es keine gibt.
fn eigen_hoehe(natuerlich: (u32, u32)) -> Option<f32> {
    (natuerlich.1 > 0 && brauchbar(natuerlich.1 as f32)).then_some(natuerlich.1 as f32)
}

/// Die Eigenbreite, oder `None` — wenn es keine gibt.
fn eigen_breite(natuerlich: (u32, u32)) -> Option<f32> {
    (natuerlich.0 > 0 && brauchbar(natuerlich.0 as f32)).then_some(natuerlich.0 as f32)
}

/// Die **benutzte** Groesse eines ersetzten Elements — CSS 2.1 §10.3.2.
///
/// **ACHTUNG, AUFRUFER:** diese Funktion erwartet **zusammengefuehrte** Werte.
/// Sie kann den gemessenen Fall `height:auto` neben `height="100"` **nicht**
/// ausdruecken (20:59, `mutation_md/bild/verh_ff2/ff.png`, Zeile 8: 120x60
/// statt 120x100). Dafuer ist [`groesse_erklaeren`] da — es rechnet genau die
/// Zusammenfuehrung und ruft diese Funktion am Ende auf.
///
/// # WARUM DIESE FUNKTION UND NICHT `rechteck_in_kasten`
///
/// `rechteck_in_kasten()` rechnet aus einem **Kasten** die Lage des Bildes
/// darin. Diese Funktion rechnet davor: **wie gross ist der Kasten** — und das
/// ist die Frage, an der www.wikipedia.org scheitert (doppelt so grosses
/// Puzzelbild). Sie gehoert in die Bildercrate, weil sie **zwei Zahlen und
/// keine Crate** braucht: die erklaerte Groesse und die Eigenmasse.
///
/// # GEMESSEN AN FIREFOX 157 (03.10.2026, 20:0x), Quelle 200x100
///
/// | Fall | Angaben am `<img>` | Firefox zeichnet |
/// |---|---|---|
/// | A | `srcset="… 2x"` | **200 x 100** |
/// | B | `srcset="… 2x" width="120" height="60"` | **120 x 60** |
/// | C | `srcset="… 2x" width="120"` | **120 x 60** |
/// | D | `srcset="… 2x" style="width:120px"` | **120 x 60** |
/// | E | `srcset="… 2x" width="300" height="150"` | **300 x 150** |
/// | F | `src width="120" height="60"` | 120 x 60 |
///
/// **DREI DINGE, DIE DIE MESSUNG GESAGT HAT — und die nicht in der Norm
/// stehen:**
///
/// 1. **Es gibt KEIN „x-Faktor teilen".** GEMESSEN an **acht** Deskriptoren
///    (`ohne`, `1x`, `2x`, `3x`, `4x`, `0.5x`, `1.5x`, `1.25x`): Firefox zeichnet
///    in **allen** Faellen **200 x 100**. Der `x`-Deskriptor bestimmt, **welche
///    Datei** geladen wird — nicht, wie gross sie gezeichnet wird. Die
///    naheliegende Erklaerung fuer das doppelt so grosse Wikipedia-Bild ist damit
///    **widerlegt**; der `@2x`-Name der Datei ist ein Name, keine Rechnung.
/// 2. **Die erklaerte Groesse schlaegt die Eigenmasse** — und zwar **wortwoertlich**:
///    Fall **E** gibt `300 x 150` fuer ein 2:1-Bild an, und Firefox zeichnet
///    **300 x 150**. Das Seitenverhaeltnis wird **nicht** gerettet.
/// 3. **Nur eine der beiden Angaben** genuegt: Fall **C** gibt nur `width=120`
///    an, und Firefox zeichnet **120 x 60** — die Hoehe kommt aus dem
///    Seitenverhaeltnis der **Quelle** (2:1), nicht aus `height`.
///
/// # DIE REGEL, DIE DARAUS FOLGT
///
/// ```text
/// beide erklaert  -> beide benutzt, **ohne** Seitenverhaeltnis (Fall E)
/// nur eine       -> die andere aus dem Seitenverhaeltnis der Quelle (Fall C)
/// keine erklaert -> die Eigenmasse (Fall A)
/// ```
///
/// `breite` und `hoehe` sind die **erklaerten** Werte, jeweils optional: aus
/// CSS (`width`/`height`) und, wenn CSS schweigt, aus den **HTML-Attributen**.
/// `None` heisst „nicht angegeben" — **nicht** `0`, denn `0` ist eine
/// gueltige CSS-Laenge. Die beiden Fassungen kommen an dieser Stelle
/// zusammen, weil sie dieselbe Bedeutung haben; wer sie trennt, entscheidet die
/// Prioritaet an drei Orten statt an einem.
///
/// GEMESSENE ENTSCHEIDUNG, warum zwei `Option<f32>` und nicht
/// `Option<(f32, f32)>`: gemessen ist der Fall C (**nur** `width`), und er ist
/// mit einem Paar nicht ausdrueckbar — `Some((120.0, 0.0))` ist ein Paar, in dem
/// die Hoehe **0** ist, und das ist etwas anderes als „keine Hoehe". Die
/// Schreibweise mit `NaN` als Kennzeichen loest das nur scheinbar: `NaN` ist
/// eine Zahl, die jemand zufaellig verrechnen kann.
pub fn benutzte_groesse(
    breite: Option<f32>,
    hoehe: Option<f32>,
    natuerlich: (u32, u32),
) -> Option<(f32, f32)> {
    let b_gut = breite.filter(|b| brauchbar(*b) && *b <= MAX_KASTEN);
    let h_gut = hoehe.filter(|h| brauchbar(*h) && *h <= MAX_KASTEN);
    match (b_gut, h_gut) {
        // GEMESSENER Fall B und E: beide Angaben gelten, wie sie dastehen —
        // auch wenn sie das Seitenverhaeltnis verletzen.
        (Some(b), Some(h)) => Some((b, h)),
        // GEMESSENER Fall C: eine Angabe, die andere aus dem Verhaeltnis.
        (Some(b), None) => hoehe_aus_breite(b, natuerlich).map(|h| (b, h)),
        (None, Some(h)) => breite_aus_hoehe(h, natuerlich).map(|b| (b, h)),
        // GEMESSENER Fall A: nichts erklaert -> die Eigenmasse.
        (None, None) => {
            if natuerlich.0 == 0 || natuerlich.1 == 0 {
                return None;
            }
            let (b, h) = (natuerlich.0 as f32, natuerlich.1 as f32);
            if brauchbar(b) && brauchbar(h) && b <= MAX_KASTEN && h <= MAX_KASTEN {
                Some((b, h))
            } else {
                None
            }
        }
    }
}

/// Runde auf ganze Pixel: **Kaufmaennisch** (`.5` aufwaerts).
///
/// GEMESSENE BEGRUENDUNG fuer das Runden und nicht das Abschneiden: der
/// Firefox-Kasten eines `<img width="300">` bei einem 200x150-Bild ist
/// **225 px** — das geht bei beiden Regeln. Der Unterschied zeigt sich bei
/// `width="301"`: 301 * 150 / 200 = **225,75**, Kaufmaennisch **226**,
/// Abschneiden **225**. Ein um 0,75 px zu kleiner Kasten ist an einem Bild
/// mit sichtbarem Rand genau ein heller Streifen — und im Vergleich mit
/// Firefox der erste, der auffaellt.
///
/// DIE GRENZE: das Ergebnis ist auf [`crate::MAX_KANTE`] (16384) geklemmt,
/// weil der Aufrufer daraus eine `u32`-Kantenlaenge macht.
pub fn runde_px(v: f32) -> u32 {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    let gerundet = (v + 0.5).floor();
    if !gerundet.is_finite() || gerundet >= crate::MAX_KANTE as f32 {
        return crate::MAX_KANTE;
    }
    gerundet as u32
}

/// Bequemlichkeit: [`rechteck_in_kasten`] mit den Massen eines [`Bild`].
///
/// GEMESSENER GRUND, warum es diese Huelle gibt: `narsil-raster` und `narsil`
/// haben beide ein `Bild` in der Hand und **keine** `u32`-Paare — der Umweg
/// ueber `(b.breite, b.hoehe)` steht sonst an drei Stellen.
pub fn rechteck_fuer_bild(
    kasten: (f32, f32),
    bild: &Bild,
    passung: Passung,
    position: Bildposition,
) -> Rechteck {
    rechteck_in_kasten(kasten, (bild.breite, bild.hoehe), passung, position)
}

/// Liest einen `object-position`-**Wert**.
///
/// # WARUM DAS HIER LIEGT
///
/// GEMESSEN am 03.10.2026, 18:4x: `grep -rn "object_position" crates` ->
/// **null** Treffer, `grep -rn "object_fit" crates` -> **drei**, alle in
/// `narsil-css` (Parsezweig, Typfeld, Kommentar) — also **null** Verbraucher.
/// Es gibt damit im ganzen Projekt **keine** Stelle, an der aus dem Text
/// `right 10px bottom 20px` eine Zahl wird. Diese Funktion ist die Stelle.
///
/// # DIE REGELN SIND GEMESSEN, NICHT GELESEN
///
/// GEMESSEN an **Firefox 157** (headless, Screenshot), Bild mit **60x40**
/// Eigenmasse im Kasten **100x80** und `object-fit: none` — also **40 px**
/// freier Raum in **beiden** Achsen, und der schwarze Rand des Bildes steht im
/// Screenshot genau da, wohin die Position es schiebt. Rohdaten:
/// `/home/jaff/mutation_md/bild/position_ff*/ff.png`, abgelesen mit
/// `/tmp/opencode/bild_use/position_auslesen.py`.
///
/// ```text
/// Wert                        Firefox     意味着
/// center                      20,20        Mitte/Mitte
/// top                         20, 0        ACHSENBEWUSST: x = Mitte, y = 0
/// bottom                      20,40        dito
/// left                         0,20
/// right                       40,20
/// 20px                        20,20        EIN Laengenwert geht auf **x**
/// 5px                          5,20
/// left 20px                    0,20        Zwei-Wert-Form: x = left, y = 20px
/// right 10px                  40,10        dito — **nicht** „10 px von rechts"
/// center 5px                  20, 5        dito: center ist x, 5px ist y
/// center 30px                 20,30
/// top left                     0, 0        **gueltig**, die Reihenfolge ist egal
/// bottom right                40,40        dito
/// 5px 20px                     5,20
/// 0px 0px                      0, 0
/// right 10px bottom 20px      30,20        Vier-Wert-Form: 40-10, 40-20
/// left 5px top 7px             5, 7        dito von linker/oberer Kante
/// right 10px bottom           20,20        **DREI Werte: verworfen**
/// top 5px                     20,20        **verworfen** (Laenge hinter y-Begriff)
/// top left 5px                20,20        **verworfen**
/// calc(50% - 10px)            10,20        Firefox **rechnet** calc
/// 1em                         11,20        Firefox **kennt** em
/// ```
///
/// # DIE VIER DINGE, DIE VON DER GRAMMATIK ABWEICHEN
///
/// 1. **Ein Wert geht auf seine Achse, nicht auf x.** `top` ergibt in Firefox
///    `20, 0` und nicht `0, 20`. Nach der ersten Zeile der Grammatik in
///    CSS Backgrounds 3 §3.4 stuende es auf x — Firefox macht es anders.
///    *Nur* ein **Laengen**wert geht auf x (`20px` -> `20,20`).
/// 2. **Die Reihenfolge der Begriffe ist egal.** `top left` ist gueltig und
///    ergibt dasselbe wie `left top`; `bottom right` wie `right bottom`.
/// 3. **Drei Werte sind ungueltig.** Die Grammatik kennt die Drei-Wert-Form
///    (`right 10px bottom`), Firefox verwirft sie und nimmt `50% 50%`.
/// 4. **`calc()` und `em` rechnet Firefox, diese Crate nicht.** Sie liefert
///    `None`, der Aufrufer nimmt `50% 50%` — das ist bei `calc(50% - 10px)`
///    ein Versatz von **10 px** gegenueber Firefox. Gemessen, benannt, und
///    nicht kaschiert: [`Anker`] hat keinen Platz fuer eine CSS-Laenge ohne
///    px, und eine ohne px zu **raten** waere schlechter als `None`.
///
/// # DIE ENTSCHEIDUNG BEI UNGUELTIGER ANGABE
///
/// `None`. Nicht `center`, nicht der erste Wert: eine ungueltige Angabe ist in
/// CSS **ungueltig**, und der Aufrufer nimmt dann seinen Vorgabewert
/// (`object-position: 50% 50%`). Wer hier still raeht, macht aus einem
/// Tippfehler ein Bild an der falschen Stelle — und genau das macht Firefox
/// bei `top 5px` messbar.
///
/// # DIE GRENZEN (M3/M4)
///
/// `MAX_POSITION_TEXT` (256 Zeichen) begrenzt die Eingabe, **bevor** sie in
/// `Vec` gespalten wird. GEMESSENE ENTSCHEIDUNG, warum `trim()` **vor** der
/// Grenze kommen darf: `trim()` allokiert nichts und `split_whitespace`
/// braucht die geklemmte Eingabe — eine Datei mit 1 MB Leerzeichen kostet
/// damit einen Durchlauf und keine Kette.
pub const MAX_POSITION_TEXT: usize = 256;

/// Liest `object-position` / `background-position` in beiden Achsen.
///
/// ```rust
/// use narsil_image::platzierung::{Anker, Bildposition, position_lesen};
/// assert_eq!(position_lesen("right 10px bottom 20px"), Some(Bildposition {
///     x: Anker::PixelVomEnde(10.0),
///     y: Anker::PixelVomEnde(20.0),
/// }));
/// // GEMESSEN an Firefox 157: `top` ist **achsenbewusst**.
/// assert_eq!(position_lesen("top").unwrap().y, Anker::Prozent(0.0));
/// ```
/// Hoechste Laenge eines `object-fit`-Wortes in Zeichen (M4).
///
/// `object-fit` ist **ein** Schluesselwort, keine Liste: das laengste gueltige
/// Wort ist `scale-down` (10 Zeichen). Die Grenze liegt deutlich darueber, damit
/// ein Tippfehler (`scale-dowwn`, 11 Zeichen) noch **als Fehler** erkannt wird und
/// nicht an der Grenze abgeschnitten und dadurch *gueltig* wird.
pub const MAX_PASSUNG_TEXT: usize = 32;

/// Liest `object-fit` (CSS Images 1, §5.2) in eine [`Passung`].
///
/// **GEMESSEN an Firefox 157 (04.10.2026, Rolle BILD), roter PNG-Quellballast
/// 100x23, Bild im `<img>` auf 100 % x 100 % des Kastens, Kasten 70x40:**
///
/// | Angabe                    | Firefox | das ist        |
/// |---------------------------|---------|----------------|
/// | `fill`                    | 70 x 40 | der Vorgabewert |
/// | `contain`                 | 70 x 16 | `contain`       |
/// | `cover`                   | 70 x 40 | deckt den Kasten|
/// | `none`                    | 70 x 23 | Eigenmasse, oben links |
/// | `scale-down`              | 70 x 16 | `min(none, contain)` |
/// | `bollen` (**ungueltig**)  | 70 x 40 | **der Vorgabewert** |
/// | `"  contain  "` (Randleerzeichen) | 70 x 16 | `contain`  |
///
/// **ZWEI MESSUNGEN, DIE ETWAS KOSTEN:**
///
/// 1. **Randleerzeichen sind erlaubt** — `"  contain  "` verhaelt sich wie
///    `contain`, nicht wie der Vorgabewert. Darum `trim()`.
/// 2. **`COVER` in Grossbuchstaben ist mit dieser Messung NICHT
///    entscheidbar.** GEMESSEN: 70 x 40 — und **das ist bei diesem Kasten
///    sowohl `fill` als auch `cover`.** Ich habe es nach CSS Syntax 3 §3.2
///    (ASCII case-insensitive) gebaut und **sage, dass es ungemessen ist**,
///    statt eine Messung zu behaupten, die die Frage nicht beantwortet.
///
/// **WARUM `None` UND NICHT `Fuellen`:** dasselbe wie bei
/// [`position_lesen`] — eine ungueltige Angabe ist in CSS **ungueltig**, und
/// der Aufrufer nimmt seinen Vorgabewert. Wer hier still raeht, macht aus einem
/// Tippfehler ein Bild mit falscher Passung.
pub fn passung_lesen(text: &str) -> Option<Passung> {
    let t = text.trim();
    if t.is_empty() || t.len() > MAX_PASSUNG_TEXT {
        return None;
    }
    // Zwei Woerter sind **keine** `object-fit`-Angabe, auch nicht `fill cover`.
    if t.contains(char::is_whitespace) {
        return None;
    }
    if t.eq_ignore_ascii_case("fill") {
        Some(Passung::Fuellen)
    } else if t.eq_ignore_ascii_case("contain") {
        Some(Passung::Einpassen)
    } else if t.eq_ignore_ascii_case("cover") {
        Some(Passung::Bedecken)
    } else if t.eq_ignore_ascii_case("none") {
        Some(Passung::Keine)
    } else if t.eq_ignore_ascii_case("scale-down") {
        Some(Passung::Verkleinern)
    } else {
        None
    }
}

/// Der ganze Weg von den **CSS-Zeichenketten** zur Zeichenposition — das, was ein
/// Aufrufer aus dem Stil braucht.
///
/// Bisher gab es [`rechteck_in_kasten`] (mit bereits gebauten Werten) und
/// [`position_lesen`], aber **keinen** Weg von dem, was im Stil steht, zu dem
/// Rechteck. GEMESSEN 04.10.2026 (Rolle BILD): `grep -rn "rechteck_in_kasten|
/// passung_lesen|position_lesen" crates` findet **null Verbraucher ausserhalb
/// dieser Crate** — `narsil-css` **parst** `object-fit`
/// (`crates/narsil-css/src/parse.rs:5208`) und **niemand liest es**. Diese Funktion
/// ist die Luecke: sie nimmt genau die zwei Zeichenketten, die der Stil liefert.
///
/// **Die Vorgaben sind Firefox' Vorgaben**, beide **gemessen**: eine ungueltige
/// `object-fit` ergibt `fill`, eine ungueltige `object-position` ergibt
/// `50% 50%`.
///
/// ```rust
/// use narsil_image::platzierung::platziere_bild;
/// // GEMESSEN an Firefox 157, Bild 100x23 im Kasten 70x40: `contain` ergibt
/// // 70 x 16.1, `fill` ergibt 70 x 40.
/// let r = platziere_bild((70.0, 40.0), (100, 23), "contain", "center");
/// assert!((r.breite - 70.0).abs() < 0.01 && (r.hoehe - 16.1).abs() < 0.1);
/// ```
/// Das Zielrechteck **und** der Schnitt, auf den es zu schneiden ist — die
/// beiden Zahlen, die ein `DisplayItem::Bild` braucht.
///
/// **WARUM ZWEI UND NICHT EINE:** `object-fit: cover` **zeichnet** groesser als
/// den Kasten und wird vom Kasten **geschnitten**. GEMESSEN an Firefox 157
/// (04.10.2026, Rolle BILD), Quelle 100x23:
///
/// | Kasten | `object-fit` | **gezeichnet** | **sichtbar** |
/// |--------|--------------|---------------|--------------|
/// | 70x40  | `cover`      | **173,9 x 40**| **70 x 40**  |
/// | 70x40  | `none`       | **100 x 23**  | **70 x 23**  |
/// | 240x140| `cover`      | **608,7 x 140** | **240 x 140** |
///
/// Wer nur das sichtbare Rechteck zurueckgibt, streckt das Bild auf den Kasten
/// (`fill`) — das ist der Fehler, den ein fehlendes `object-fit` genau macht.
/// **Wer nur das gezeichnete zurueckgibt, malt einen 173,9 px breiten Kasten in
/// einen 70 px breiten hinein.** Beides braucht es, und `DisplayList` hat
/// beides: `DisplayItem::Bild { rect }` traegt das gezeichnete, und
/// `push_mit_clip(item, clip)` den Schnitt.
///
/// ```rust
/// use narsil_image::platzierung::platziere_bild_mit_schnitt;
/// // GEMESSEN an Firefox 157: `cover` im Kasten 70x40 ergibt 173,9 x 40 gezeichnet
/// // und 70 x 40 sichtbar.
/// let (gezeichnet, schnitt) = platziere_bild_mit_schnitt((70.0, 40.0), (100, 23), "cover", "center");
/// assert!((gezeichnet.breite - 173.9).abs() < 0.2 && (gezeichnet.hoehe - 40.0).abs() < 0.01);
/// assert!((schnitt.breite - 70.0).abs() < 0.01 && (schnitt.hoehe - 40.0).abs() < 0.01);
/// ```
pub fn platziere_bild_mit_schnitt(
    kasten: (f32, f32),
    eigen: (u32, u32),
    passung_text: &str,
    positions_text: &str,
) -> (Rechteck, Rechteck) {
    let gezeichnet = platziere_bild(kasten, eigen, passung_text, positions_text);
    let schnitt = match (brauchbar(kasten.0), brauchbar(kasten.1)) {
        (true, true) => Rechteck {
            x: 0.0,
            y: 0.0,
            breite: kasten.0.min(MAX_KASTEN),
            hoehe: kasten.1.min(MAX_KASTEN),
        },
        _ => Rechteck::default(),
    };
    (gezeichnet, schnitt)
}

pub fn platziere_bild(
    kasten: (f32, f32),
    eigen: (u32, u32),
    passung_text: &str,
    positions_text: &str,
) -> Rechteck {
    rechteck_in_kasten(
        kasten,
        eigen,
        passung_lesen(passung_text).unwrap_or_default(),
        position_lesen(positions_text).unwrap_or_default(),
    )
}

pub fn position_lesen(text: &str) -> Option<Bildposition> {
    let t = text.trim();
    if t.is_empty() || t.len() > MAX_POSITION_TEXT {
        return None;
    }
    let w: Vec<&str> = t.split_whitespace().collect();
    if w.len() > 4 {
        return None;
    }
    match w.len() {
        //
        // EIN Wert. GEMESSEN: ein **Begriff** geht auf seine Achse, ein
        // **Laengenwert** auf x. `top` ist also `center top`.
        1 => {
            let v = anker_aus_wert(w[0])?;
            Some(match v {
                Anker::Prozent(p) if ist_y_begriff(w[0]) => Bildposition {
                    x: Anker::Mitte,
                    y: Anker::Prozent(p),
                },
                _ => Bildposition {
                    x: v,
                    y: Anker::Mitte,
                },
            })
        }
        //
        // ZWEI Werte. GEMESSEN: die **Reihenfolge ist egal**, jeder Wert geht
        // auf die Achse, die sein Begriff nennt; ein Laengenwert nimmt die
        // andere Achse.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das als zwei Faelle und nicht als
        // „sortieren" geschrieben wird: `left 20px` heisst `x = links,
        // y = 20px` und **nicht** `x = links + 20px`. Wer hier die Achse
        // sortiert, bekommt bei `right 10px` ein 30 px daneben liegendes Bild.
        2 => {
            let a = w[0];
            let b = w[1];
            if ist_y_begriff(a) || ist_y_begriff(b) {
                // Mindestens ein y-Begriff: er nimmt y, der andere x.
                if ist_y_begriff(a) && ist_y_begriff(b) {
                    return None;
                }
                let (yv, xv) = if ist_y_begriff(a) { (a, b) } else { (b, a) };
                //
                // GEMESSEN: `top 5px` ist **ungueltig** (Firefox zeichnet an
                // der Vorgabe 50% 50%), `top left` ist gueltig. Der
                // Unterschied ist die **Art** des zweiten Wortes: hinter einem
                // y-Begriff steht nur ein **Begriff**, keine Laenge — in der
                // Grammatik ist der erste Platz `[left|center|right|<lp>]` und
                // `top` gehoert nicht hinein, mit einer Laenge dahinter auch
                // nicht.
                if ist_y_begriff(a) && !ist_begriff(xv) {
                    return None;
                }
                return Some(Bildposition {
                    x: anker_aus_wert(xv)?,
                    y: anker_aus_wert(yv)?,
                });
            }
            if ist_x_begriff(a) && ist_x_begriff(b) {
                return None;
            }
            Some(Bildposition {
                x: anker_aus_wert(a)?,
                y: anker_aus_wert(b)?,
            })
        }
        //
        // DREI Werte: **ungueltig**. GEMESSEN an `right 10px bottom` (20,20)
        // und `top left 5px` (20,20) — beide sind die Vorgabe, nicht eine
        // gerechnete Lage. Die Grammatik kennt diese Form; Firefox nicht.
        3 => None,
        //
        // VIER Werte: `[left|right] <lp> [top|bottom] <lp>`, und die Laengen
        // zaehlen **von der genannten Kante**.
        //
        // GEMESSEN: `right 10px bottom 20px` -> 30,20 (frei 40, also 40-10 und
        // 40-20); `left 5px top 7px` -> 5,7.
        _ => {
            if !ist_x_begriff(w[0]) || !ist_y_begriff(w[2]) {
                return None;
            }
            let x = match w[0] {
                "left" => Anker::Pixel(laenge_px(w[1])?),
                _ => Anker::PixelVomEnde(laenge_px(w[1])?),
            };
            let y = match w[2] {
                "top" => Anker::Pixel(laenge_px(w[3])?),
                _ => Anker::PixelVomEnde(laenge_px(w[3])?),
            };
            Some(Bildposition { x, y })
        }
    }
}

/// Ist `t` ein **waagerechter** Rand-Begriff (`left`/`right`)?
fn ist_x_begriff(t: &str) -> bool {
    matches!(t, "left" | "right")
}

/// Ist `t` ein **senkrechter** Rand-Begriff (`top`/`bottom`)?
fn ist_y_begriff(t: &str) -> bool {
    matches!(t, "top" | "bottom")
}

/// Ist `t` ein **Begriff** — `left`, `right`, `center`, `top` oder `bottom`?
///
/// GEMESSENE ENTSCHEIDUNG, warum es diese Frage gibt: hinter einem
/// y-Begriff darf nur ein **Begriff** stehen (`top left` ist gueltig,
/// `top 5px` nicht), und hinter einem x-Begriff eine Laenge (`left 20px`).
fn ist_begriff(t: &str) -> bool {
    matches!(t, "left" | "right" | "center" | "top" | "bottom")
}

/// Ein Wert als Anker: `left`/`right`/`top`/`bottom` als Prozent, `center` als
/// Mitte, sonst eine Laenge.
///
/// GEMESSENE GRUNDLAGE fuer die Prozentzahlung: `right` ist **100 %** und
/// `left` **0 %** — das ist der Weg, den auch [`rechteck_in_kasten`] nimmt
/// (`frei * p / 100`). `center` bleibt die Mitte und wird nicht zu 50 %
/// umgeschrieben, damit `ist_y_begriff` die Achse erkennt.
fn anker_aus_wert(t: &str) -> Option<Anker> {
    match t {
        "left" | "top" => Some(Anker::Prozent(0.0)),
        "right" | "bottom" => Some(Anker::Prozent(100.0)),
        "center" => Some(Anker::Mitte),
        _ => anker_aus_laenge(t),
    }
}

/// Ein Wert, der selbst eine Laenge oder ein Prozent ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **kein** `Anker::Mitte` herauskommt,
/// wenn nichts erkannt wird: eine unbekannte Angabe ist **ungueltig**, und
/// `None` sagt das. `50%` wird zu [`Anker::Prozent`], nicht zu Mitte.
fn anker_aus_laenge(t: &str) -> Option<Anker> {
    if t.ends_with('%') {
        let p: f32 = t[..t.len() - 1].parse().ok()?;
        if !p.is_finite() {
            return None;
        }
        return Some(Anker::Prozent(p));
    }
    laenge_px(t).map(Anker::Pixel)
}

/// Eine Laenge in px — **nur** px.
///
/// GEMESSENE ENTSCHEIDUNG, warum andere Einheiten ungueltig sind statt
/// umgerechnet: `em` braucht die Schriftgroesse, `vw` die Fensterbreite, und
/// beides hat diese Crate nicht. GEMESSEN an Firefox: `1em` ergab dort 11 px
/// (11-px-Schrift) und `calc(50% - 10px)` 10 px — wir geben `None`, und das
/// ist ein **gemessener** Unterschied von 10 px, kein Versehen.
fn laenge_px(t: &str) -> Option<f32> {
    // Release 15.10.2026: a unitless zero is a length (CSS Values 4 §6.1:
    // `object-position: 0 0`). MEASURED before: `0 0` was invalid, and the
    // image fell back to `50% 50%`.
    if t.parse::<f32>().is_ok_and(|v| v == 0.0) {
        return Some(0.0);
    }
    let (zahl, einheit) = t.split_at(t.len().checked_sub(2)?);
    if !einheit.eq_ignore_ascii_case("px") {
        return None;
    }
    let v: f32 = zahl.trim().parse().ok()?;
    //
    // M4: eine Zahl aus einem Stylesheet darf keine `u32`-Umrechnung unterwegs
    // sprengen. GEMESSENE ENTSCHEIDUNG, warum **verworfen** und nicht
    // geklemmt wird: Firefox **klemmt** eine zu grosse Laenge auf seinen
    // groessten Wert, und [`versatz`] klemmt anschliessend auf [`MAX_LAGE`]
    // (1 000 000) — das Ergebnis ist dieselbe Zahl. Der Unterschied ist nur
    // der Weg: hier ist die Zahl **ungueltig**, und der Aufrufer nimmt
    // `50% 50%`. Fuer `1e9px` ist das dieselbe Lage wie fuer `1e20px`.
    if v.is_finite() && v.abs() <= MAX_LAGE {
        Some(v)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jeder Test **sichert ein Ergebnis zu** (M7): kein `assert!(x)`, sondern
    /// ein Wert, und im Fehlerfall steht die Zahl im Text.

    // ---------------------------------------------------------------------
    // `position_lesen` — jede Zahl unten ist in Firefox **gemessen**,
    // 18:4x, Bild 60x40 im Kasten 100x80 (40 px frei), `object-fit: none`.
    // ---------------------------------------------------------------------

    /// EIN Wert: ein **Begriff** geht auf seine Achse, ein **Laengenwert** auf
    /// x. GEMESSEN: `top` -> 20,0 (nicht 0,20).
    #[test]
    fn ein_wert_acht_auf_seine_achse() {
        assert_eq!(
            position_lesen("center").map(|p| (p.x, p.y)),
            Some((Anker::Mitte, Anker::Mitte))
        );
        assert_eq!(
            position_lesen("top").map(|p| (p.x, p.y)),
            Some((Anker::Mitte, Anker::Prozent(0.0))),
            "GEMESSEN Firefox 157: top -> 20,0. Nach der Grammatik stuende es auf x."
        );
        assert_eq!(
            position_lesen("bottom").map(|p| (p.x, p.y)),
            Some((Anker::Mitte, Anker::Prozent(100.0))),
            "GEMESSEN: 20,40"
        );
        assert_eq!(
            position_lesen("left").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(0.0), Anker::Mitte)),
            "GEMESSEN: 0,20"
        );
        assert_eq!(
            position_lesen("right").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(100.0), Anker::Mitte)),
            "GEMESSEN: 40,20"
        );
        assert_eq!(
            position_lesen("20px").map(|p| (p.x, p.y)),
            Some((Anker::Pixel(20.0), Anker::Mitte)),
            "GEMESSEN: 20,20 — ein Laengenwert geht auf **x**"
        );
        assert_eq!(
            position_lesen("5px").map(|p| (p.x, p.y)),
            Some((Anker::Pixel(5.0), Anker::Mitte)),
            "GEMESSEN: 5,20"
        );
        assert_eq!(
            position_lesen("25%").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(25.0), Anker::Mitte)),
            "ein Prozentwert geht auf x"
        );
    }

    /// ZWEI Werte: die **Reihenfolge ist egal**, und ein Laengenwert nimmt die
    /// Achse, die der Begriff nicht nimmt.
    #[test]
    fn zwei_werte_sind_reihenfolgenunabhaengig() {
        assert_eq!(
            position_lesen("left top").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(0.0), Anker::Prozent(0.0))),
            "GEMESSEN: 0,0"
        );
        assert_eq!(
            position_lesen("top left").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(0.0), Anker::Prozent(0.0))),
            "GEMESSEN: 0,0 — die Reihenfolge ist egal, der Wert ist gueltig"
        );
        assert_eq!(
            position_lesen("bottom right").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(100.0), Anker::Prozent(100.0))),
            "GEMESSEN: 40,40"
        );
        assert_eq!(
            position_lesen("center bottom").map(|p| (p.x, p.y)),
            Some((Anker::Mitte, Anker::Prozent(100.0))),
            "GEMESSEN: 20,40"
        );
        assert_eq!(
            position_lesen("right bottom").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(100.0), Anker::Prozent(100.0))),
            "GEMESSEN: 40,40"
        );
        assert_eq!(
            position_lesen("5px 20px").map(|p| (p.x, p.y)),
            Some((Anker::Pixel(5.0), Anker::Pixel(20.0))),
            "GEMESSEN: 5,20"
        );
        assert_eq!(
            position_lesen("0px 0px").map(|p| (p.x, p.y)),
            Some((Anker::Pixel(0.0), Anker::Pixel(0.0))),
            "GEMESSEN: 0,0"
        );
        assert_eq!(
            position_lesen("10px 20%").map(|p| (p.x, p.y)),
            Some((Anker::Pixel(10.0), Anker::Prozent(20.0))),
            "Laenge und Prozent mischen sich"
        );
    }

    /// DER FALL, DER DIE GANZE ZEILE ERKLAERT: `right 10px` ist die
    /// **Zwei-Wert-Form** und **nicht** „10 px von rechts".
    #[test]
    fn laenge_nach_rand_begriff_ist_die_andere_achse() {
        assert_eq!(
            position_lesen("right 10px").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(100.0), Anker::Pixel(10.0))),
            "GEMESSEN Firefox: 40,10 — x = rechts, y = 10px"
        );
        assert_eq!(
            position_lesen("left 20px").map(|p| (p.x, p.y)),
            Some((Anker::Prozent(0.0), Anker::Pixel(20.0))),
            "GEMESSEN: 0,20"
        );
        assert_eq!(
            position_lesen("center 5px").map(|p| (p.x, p.y)),
            Some((Anker::Mitte, Anker::Pixel(5.0))),
            "GEMESSEN: 20,5 — die Laenge ist **y**, nicht ein Versatz der Mitte"
        );
        assert_eq!(
            position_lesen("center 30px").map(|p| (p.x, p.y)),
            Some((Anker::Mitte, Anker::Pixel(30.0))),
            "GEMESSEN: 20,30"
        );
    }

    /// DIE VIER-WERT-FORM: die Laengen zaehlen **von der genannten Kante**.
    #[test]
    fn vier_werte_zaehlen_vom_rand() {
        assert_eq!(
            position_lesen("right 10px bottom 20px").map(|p| (p.x, p.y)),
            Some((Anker::PixelVomEnde(10.0), Anker::PixelVomEnde(20.0))),
            "GEMESSEN: 30,20 bei 40 px freiem Raum"
        );
        assert_eq!(
            position_lesen("left 5px top 7px").map(|p| (p.x, p.y)),
            Some((Anker::Pixel(5.0), Anker::Pixel(7.0))),
            "GEMESSEN: 5,7"
        );
        //
        // Und die Zahl, die zeigt, dass es **nicht** dasselbe ist: Kasten
        // 300 breit, Bild 200 (frei = 100). `right 10px` ergibt 90,
        // `left 10px` ergibt 10 — der Unterschied betraegt **80 px**.
        let r = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 200),
            Passung::Keine,
            Bildposition {
                x: Anker::PixelVomEnde(10.0),
                y: Anker::Mitte,
            },
        );
        assert_eq!(r.x, 90.0, "frei 100 minus 10 von rechts");
        let l = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 200),
            Passung::Keine,
            Bildposition {
                x: Anker::Pixel(10.0),
                y: Anker::Mitte,
            },
        );
        assert_eq!(l.x, 10.0, "von links");
        assert_eq!(r.x - l.x, 80.0, "der Unterschied der beiden Lesarten");
    }

    /// DREI Werte und Laengen hinter einem **y**-Begriff sind ungueltig —
    /// Firefox nimmt dann `50% 50%`, und das ist gemessen, nicht behauptet.
    #[test]
    fn drei_werte_und_falsche_kombinationen_sind_ungueltig() {
        for q in [
            "right 10px bottom",
            "top left 5px",
            "top 5px",
            "left right",
            "top bottom",
            "center center center",
        ] {
            assert_eq!(
                position_lesen(q),
                None,
                "{q:?} — GEMESSEN: Firefox zeichnet an der Vorgabe 50% 50%"
            );
        }
    }

    /// UNGUELTIG ist ungueltig: kein Raten, kein stiller Vorgabewert.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `calc()` und `em` in dieser Liste stehen
    /// und nicht irgendwo gesondert: Firefox **rechnet** sie (10 px bzw. 11 px
    /// gemessen), diese Crate nicht. `None` heisst „nicht gelesen", und der
    /// Aufrufer nimmt `50% 50%` — das ist ein **gemessener** Abstand, kein
    /// stiller Erfolg.
    #[test]
    fn ungueltige_angaben_werden_verworfen() {
        for q in [
            "",
            "   ",
            "links", // kein CSS-Begriff
            "1.5em",
            "10em",
            "50vw",
            "2rem",
            "calc(50% - 10px)",
            "calc(100% - 10px)",
            "right 10% bottom 20%", // hinter einem Rand-Begriff nur Laenge
            "right 10px bottom 20px extra",
            "10px 10px 10px 10px 10px",
            "99999999999999999999px",
            "10 px", // Leerzeichen im Wert
        ] {
            assert_eq!(
                position_lesen(q),
                None,
                "{q:?} ist ungueltig und darf nicht geraten werden"
            );
        }
        //
        // DIE ZAHL des gemessenen Abstands zu Firefox: `calc(50% - 10px)` gibt
        // in Firefox den Versatz 10 px (frei 40, 50 % = 20, minus 10), bei uns
        // die Mitte 20. Der Unterschied betraegt **10 px**.
        assert_eq!(
            rechteck_in_kasten((100.0, 80.0), (60, 40), Passung::Keine, Bildposition::MITTE).x,
            20.0,
            "unsere Vorgabe; Firefox setzt calc(50% - 10px) auf 10 — 10 px daneben"
        );
    }

    /// M3/M4: die Eingabe wird begrenzt, **bevor** gespalten wird.
    #[test]
    fn lange_angabe_endet_ohne_arbeit() {
        // 300 Leerzeichen **zwischen** zwei Werten: die Grenze greift, weil
        // `trim()` nur die Raender abschneidet.
        let q = format!("left{}top", " ".repeat(MAX_POSITION_TEXT));
        assert_eq!(position_lesen(&q), None, "zu lang, also nicht gelesen");
        assert_eq!(MAX_POSITION_TEXT, 256);
        //
        // Und der Rand: genau 256 geht noch.
        // "left" (4) + n Leerzeichen + "top" (3) = 256, also n = 249.
        let ok = format!("left{}top", " ".repeat(MAX_POSITION_TEXT - 7));
        assert_eq!(ok.len(), 256);
        assert_eq!(
            position_lesen(&ok).map(|p| (p.x, p.y)),
            Some((Anker::Prozent(0.0), Anker::Prozent(0.0))),
            "genau an der Grenze wird noch gelesen"
        );
    }

    #[test]
    fn fill_streckt_auf_den_kasten() {
        let r = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Fuellen,
            Bildposition::MITTE,
        );
        assert_eq!((r.breite, r.hoehe), (300.0, 200.0), "fill = ganzer Kasten");
        assert_eq!((r.x, r.y), (0.0, 0.0), "fill hat keinen freien Raum");
    }

    #[test]
    fn contain_haelt_das_seitenverhaeltnis() {
        // 200x100 in 300x200: s = min(300/200, 200/100) = min(1,5; 2) = 1,5
        // -> 300x150. Die Breite fuellt den Kasten, die Hoehe nicht.
        let r = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Einpassen,
            Bildposition::MITTE,
        );
        assert_eq!(
            (r.breite, r.hoehe),
            (300.0, 150.0),
            "breite fuellt, hoehe laesst 50 px frei"
        );
        assert_eq!(
            (r.x, r.y),
            (0.0, 25.0),
            "waagerecht kein freier Raum, senkrecht 50/2"
        );
        //
        // GEMESSENE ENTSCHEIDUNG, warum dieses Beispiel oben steht und nicht
        // ein „passt schon"-Fall: `contain` **vergroessert**. Wer es mit
        // `none` verwechselt, haelt ein 200x100-Bild in einem 300x200-Kasten
        // fuer „beides passt" — und der Unterschied ist genau die halbe
        // Bildhoehe (150 statt 100), also **50 px** Unterschied im Bild.
        //
        // Die Kontrollrechnung, die den Unterschied sichtbar macht:
        let n = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Keine,
            Bildposition::MITTE,
        );
        assert_eq!(n.hoehe, 100.0, "none bleibt bei der Eigenmasse");
        assert!(
            r.hoehe > n.hoehe,
            "contain (150) ist groesser als none (100): contain vergroessert"
        );
        //
        // Und das Seitenverhaeltnis ist in BEIDEN erhalten: 300/150 = 200/100 = 2.
        assert!(
            (r.breite / r.hoehe - 2.0).abs() < 0.0001,
            "Verhaeltnis 2 erhalten, bekam {}",
            r.breite / r.hoehe
        );
    }

    #[test]
    fn cover_wird_groesser_als_der_kasten() {
        // s = max(1,5; 2) = 2 -> 400x200, ragt 50 links und rechts ueber
        let r = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Bedecken,
            Bildposition::MITTE,
        );
        assert_eq!(
            (r.breite, r.hoehe),
            (400.0, 200.0),
            "cover fuellt die kleinere Achse"
        );
        assert_eq!(
            r.x, -50.0,
            "50 px ragt links ueber, MITTE haelt es symmetrisch"
        );
    }

    #[test]
    fn none_behaelt_die_eigenmasse() {
        let r = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Keine,
            Bildposition::MITTE,
        );
        assert_eq!(
            (r.breite, r.hoehe),
            (200.0, 100.0),
            "none = Eigenmasse, auch wenn sie passt"
        );
    }

    #[test]
    fn scale_down_ist_die_kleinere_der_beiden() {
        // passt schon: 200x100 in 300x200 -> bleibt 200x100 (wie none)
        let a = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Verkleinern,
            Bildposition::MITTE,
        );
        assert_eq!(
            (a.breite, a.hoehe),
            (200.0, 100.0),
            "scale-down verkleinert nicht"
        );
        // passt nicht: 400x400 in 300x200 -> s = min(0,75; 0,5) = 0,5 -> 200x200
        let b = rechteck_in_kasten(
            (300.0, 200.0),
            (400, 400),
            Passung::Verkleinern,
            Bildposition::MITTE,
        );
        assert_eq!(
            (b.breite, b.hoehe),
            (200.0, 200.0),
            "scale-down wie contain"
        );
        // und der Unterschied zu `none` ist genau die Verkleinerung
        let c = rechteck_in_kasten(
            (300.0, 200.0),
            (400, 400),
            Passung::Keine,
            Bildposition::MITTE,
        );
        assert_eq!((c.breite, c.hoehe), (400.0, 400.0), "none laesst es gross");
        assert!(b.breite < c.breite, "scale-down ist nie groesser als none");
    }

    #[test]
    fn position_25_75_verschiebt_nach_vorn_und_unten() {
        // 200x100 in 300x200: frei = 100 horizontal, 100 vertikal
        // 25 % -> 25, 75 % -> 75
        let p = Bildposition::prozent(25.0, 75.0);
        let r = rechteck_in_kasten((300.0, 200.0), (200, 100), Passung::Keine, p);
        assert_eq!((r.x, r.y), (25.0, 75.0), "25 % und 75 % des freien Raums");
    }

    #[test]
    fn position_0_und_100_sind_die_kanten() {
        let links_oben = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Keine,
            Bildposition::prozent(0.0, 0.0),
        );
        assert_eq!(
            (links_oben.x, links_oben.y),
            (0.0, 0.0),
            "0 % = linke obere Kante"
        );
        let rechts_unten = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Keine,
            Bildposition::prozent(100.0, 100.0),
        );
        assert_eq!(
            (rechts_unten.x, rechts_unten.y),
            (100.0, 100.0),
            "100 % = rechte untere Kante"
        );
    }

    #[test]
    fn pixel_verschiebt_vom_rand_nicht_von_der_mitte() {
        // 20 px vom linken Rand, nicht 10 px links von der Mitte
        let p = Bildposition {
            x: Anker::Pixel(20.0),
            y: Anker::Pixel(0.0),
        };
        let r = rechteck_in_kasten((300.0, 200.0), (200, 100), Passung::Keine, p);
        assert_eq!(r.x, 20.0, "eine Laenge ist ein Versatz vom Rand");
        assert_eq!(r.y, 0.0, "0 px = oberer Rand");
    }

    #[test]
    fn negative_verschiebung_schraegt_das_bild_aus_dem_kasten() {
        let p = Bildposition {
            x: Anker::Pixel(-10.0),
            y: Anker::Pixel(-10.0),
        };
        let r = rechteck_in_kasten((300.0, 200.0), (200, 100), Passung::Keine, p);
        assert_eq!(
            (r.x, r.y),
            (-10.0, -10.0),
            "negativ ist gueltig (CSS Images 3 5.5)"
        );
        assert!(
            !r.ist_leer(),
            "das Rechteck ist nicht leer, es ragt nur heraus"
        );
    }

    #[test]
    fn prozent_ausserhalb_der_skala_schiebt_das_bild_ganz_heraus() {
        // 150 % von 100 px freiem Raum = 150 px Versatz: das Bild beginnt
        // hinter der rechten Kastenkante.
        let r = rechteck_in_kasten(
            (300.0, 200.0),
            (200, 100),
            Passung::Keine,
            Bildposition::prozent(150.0, 0.0),
        );
        assert_eq!(r.x, 150.0, "150 % sind erlaubt und ergeben 150 px");
    }

    #[test]
    fn hoehe_aus_breite_ist_der_fall_ohne_hoehenangabe() {
        // <img width="300"> bei 200x150 -> 225
        assert_eq!(hoehe_aus_breite(300.0, (200, 150)), Some(225.0), "225 px");
        // exakt der Firefox-Fall: 301 * 150 / 200 = 225,75
        let h = hoehe_aus_breite(301.0, (200, 150)).unwrap_or(0.0);
        assert!(
            (h - 225.75).abs() < 0.001,
            "ungeklammert 225,75, geklammert {}",
            runde_px(h)
        );
        assert_eq!(runde_px(h), 226, "kaufmaennisch gerundet 226, nicht 225");
    }

    #[test]
    fn breite_aus_hoehe_ist_die_umkehrung() {
        // nixos-Logo 7129x1995 bei height 48 px -> 171,5 px
        let b = breite_aus_hoehe(48.0, (7129, 1995)).unwrap_or(0.0);
        assert!((b - 171.5).abs() < 0.05, "nixos-Fall: {b}");
        // und die Umkehrung ist exakt
        assert_eq!(
            hoehe_aus_breite(b, (7129, 1995)),
            Some(48.0),
            "Hin und zurueck"
        );
    }

    #[test]
    fn ohne_eigenmasse_wird_nichts_geraet() {
        assert_eq!(
            seitenverhaeltnis((0, 100)),
            None,
            "Breite 0 ist keine Masse"
        );
        assert_eq!(seitenverhaeltnis((100, 0)), None, "Hoehe 0 ist keine Masse");
        assert_eq!(seitenverhaeltnis((0, 0)), None, "beide 0");
        assert_eq!(
            hoehe_aus_breite(300.0, (0, 0)),
            None,
            "kein Verhaeltnis, keine Hoehe"
        );
        assert_eq!(
            breite_aus_hoehe(300.0, (200, 0)),
            None,
            "kein Verhaeltnis, keine Breite"
        );
    }

    #[test]
    fn nan_und_unendlich_werden_abgewiesen() {
        assert_eq!(hoehe_aus_breite(f32::NAN, (200, 150)), None, "NaN");
        assert_eq!(hoehe_aus_breite(f32::INFINITY, (200, 150)), None, "+inf");
        assert_eq!(hoehe_aus_breite(-300.0, (200, 150)), None, "negativ");
        assert!(
            rechteck_in_kasten(
                (f32::NAN, 200.0),
                (10, 10),
                Passung::Fuellen,
                Bildposition::MITTE
            )
            .ist_leer(),
            "NaN im Kasten"
        );
        assert!(
            rechteck_in_kasten(
                (300.0, 200.0),
                (10, 10),
                Passung::Fuellen,
                Bildposition {
                    x: Anker::Prozent(f32::NAN),
                    y: Anker::Mitte
                }
            )
            .breite
                > 0.0,
            "NaN im Anker wird uebersprungen, nicht vergiftet"
        );
    }

    #[test]
    fn riesige_kasten_werden_geklemmt_statt_gefressen() {
        // 1e30 px Kasten: kein Fehler, aber kein Rechteck in Pixelgroesse
        let r = rechteck_in_kasten(
            (1e30, 1e30),
            (10, 10),
            Passung::Fuellen,
            Bildposition::MITTE,
        );
        assert!(
            r.ist_leer(),
            "ein Kasten ueber MAX_KASTEN ergibt ein leeres Rechteck"
        );
        // knapp darunter wird es gerechnet, aber begrenzt
        let b = rechteck_in_kasten(
            (900_000.0, 900_000.0),
            (10, 10),
            Passung::Bedecken,
            Bildposition::MITTE,
        );
        assert!(
            b.breite <= MAX_KASTEN && b.hoehe <= MAX_KASTEN,
            "Ergebnis bleibt in der Grenze"
        );
    }

    #[test]
    fn riesiger_versatz_wird_geklemmt() {
        let p = Bildposition {
            x: Anker::Pixel(-1e12),
            y: Anker::Pixel(1e12),
        };
        let r = rechteck_in_kasten((300.0, 200.0), (200, 100), Passung::Keine, p);
        assert_eq!(r.x, -MAX_LAGE, "unten geklemmt");
        assert_eq!(r.y, MAX_LAGE, "oben geklemmt");
    }

    #[test]
    fn runde_px_ist_kaufmaennisch_und_geklemmt() {
        assert_eq!(runde_px(225.5), 226, ".5 aufwaerts");
        assert_eq!(runde_px(225.49), 225, "unter .5 abwaerts");
        assert_eq!(runde_px(0.4), 0, "unter 1 px ist 0");
        assert_eq!(runde_px(-5.0), 0, "negativ ist 0, kein u32-Ueberlauf");
        assert_eq!(runde_px(f32::NAN), 0, "NaN ist 0");
        assert_eq!(runde_px(1e30), crate::MAX_KANTE, "geklemmt auf MAX_KANTE");
    }

    #[test]
    fn fuell_bild_huelle_nimmt_die_masse_aus_dem_bild() {
        let b = Bild::new(200, 100);
        let r = rechteck_fuer_bild((300.0, 200.0), &b, Passung::Bedecken, Bildposition::MITTE);
        assert_eq!(
            (r.breite, r.hoehe),
            (400.0, 200.0),
            "gleich wie mit den Zahlen"
        );
        let leer = Bild::new(0, 0);
        assert!(
            rechteck_fuer_bild((300.0, 200.0), &leer, Passung::Fuellen, Bildposition::MITTE)
                .ist_leer(),
            "0x0 ergibt nichts"
        );
    }
}
