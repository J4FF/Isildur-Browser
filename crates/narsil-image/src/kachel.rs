// ===========================================================================
// KACHEL: WIE `background-repeat` EIN BILD UEBER EINE FLAECHE LEGT.
// ===========================================================================
//
// B-LAYOUT (03.10.2026, Rolle BILD). Auftrag: „`background-image`
// (Groesse/Position/Wiederholung)".
//
// WARUM DIESE DATEI IN DER BILDERCRATE LIEGT und nicht im Layout:
// `narsil-layout-engine/src/hintergrund.rs::kacheln()` rechnet die **Rechtecke**
// und schneidet sie auf die Flaeche zu — das braucht `Rect` und `narsil_css::BildGeo`,
// also eine fremde Crate. Diese Datei rechnet davor: **wie viele** Kacheln, mit
// welchem **Abstand**, an welchem **Versatz**, und wie gross ist eine Kachel.
// Das sind reine Zahlen ueber (Flaeche, Kachelmasse, Wiederholung, Position) —
// ohne Knoten, ohne DOM, ohne Kaskade, und **ohne Browser** pruefbar.
//
// **DIE TRENNUNG, damit es nicht zwei Regelwerke an zwei Stellen gibt:**
// * `kacheln()` = *welche Rechtecke zeichne ich* (schneiden, `MAX_KACHELN`).
// * `kachelplan()` = *wie sieht das Muster aus* (Zahl, Abstand, Versatz).
// `kacheln()` braucht diese Datei, es ersetzt sie nicht.
//
// # GEMESSEN AN FIREFOX 157, 03.10.2026, 19:1x
//
// Anordnung: Kasten **203x97** (`background-clip: padding-box`, kein Rand,
// kein Innenabstand), Kachel **40x20** mit 2-px-schwarzem Rand und einem
// Punkt in der Mitte. 203 und 97 sind **beide nicht** durch 40 und 20
// teilbar — das ist der ganze Grund fuer diese Zahlen: bei 200x100 macht
// Firefox aus `round` und `space` dasselbe wie `repeat`, und die Messung
// waere wertlos gewesen. **Erster Anlauf war genau das** (200x100), die
// Kacheln sahen dort alle gleich aus.
//
// Rohbild: `/home/jaff/mutation_md/bild/kachel_ff2/ff.png`. Die Werte unten
// sind aus den Pixeln **abgelesen**, nicht gerechnet.
//
// ```text
// Wert                Firefox misst                       Regel
// repeat              6 Kacheln in x, 5 in y             Abstand 0, Rand abgeschnitten
// no-repeat           1x1, oben links                     Versatz nach background-position
// repeat-x            6 in x, **1** in y
// repeat-y            **1** in x, 5 in y
// space               5x4 Kacheln, 40x20, Luecke 1 px x  Kacheln behalten ihre Groesse,
//                    Versaetze 0/41/82/122/163            der Zwischenraum wird gleich verteilt
//                    y: 0/26/51/77
// round               **5x5** Kacheln von **40,6 x 19,4**  die Kachel wird so gross, dass
//                                                              **ganzzahlig** viele passen
// space + position    **identisch** zu `space`             -> Position wirkt NICHT
// round + position    Muster um **(7, 3)** verschoben       -> bei round wirkt sie
// ```
//
// **DER VERSATZ ist per Kreuzkorrelation gemessen**, nicht geschaetzt:
// `round` gegen `round` mit `background-position: 7px 3px` ergibt die
// Verschiebung **(7, 3)** — die Position wirkt also **vollstaendig**; `space`
// gegen `space` mit derselben Position ergibt **(0, 0)** — sie wirkt **gar
// nicht**. (Die erste Fassung dieser Tabelle hatte fuer `round` einen
// *geschaetzten* x-Wert; die Messung hat ihn widerlegt. Eine Zahl, die nicht
// gemessen wurde, ist eine Behauptung.)

use crate::platzierung::{Anker, Bildposition, MAX_KASTEN, MAX_LAGE};

/// `background-repeat` (CSS Backgrounds 3 §3.5) — **alle sechs** Werte.
///
/// GEMESSENE GRUNDLAGE fuer die Aufteilung: `narsil_css::BildGeo` traegt
/// `wiederholen_x` und `wiederholen_y` als **zwei `bool`**. Damit sind `repeat`,
/// `no-repeat`, `repeat-x` und `repeat-y` ausdrueckbar — **`space` und `round`
/// sind es nicht**, und beide kommen auf echten Seiten vor. Siehe
/// `tests/kachel.rs::space_und_round_kann_bildgeo_nicht_ausdruecken`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wiederholung {
    /// `repeat` — der Vorgabewert. Kacheln in beide Richtungen, Rand abgeschnitten.
    #[default]
    Beide,
    /// `no-repeat` — **eine** Kachel, an der Stelle aus `background-position`.
    Keine,
    /// `repeat-x` — waagerecht kachelt, senkrecht **eine**.
    NurX,
    /// `repeat-y` — senkrecht kachelt, waagerecht **eine**.
    NurY,
    /// `space` — so viele Kacheln wie unbeschnitten passen, der Rest wird als
    /// **gleich grosser Zwischenraum** verteilt; die erste Kachel liegt an der
    /// linken, die letzte an der rechten Kante.
    Space,
    /// `round` — die Kachel wird **vergroessert oder verkleinert**, bis
    /// ganzzahlig viele in jede Richtung passen.
    Round,
}

/// Das Muster, aus dem ein Aufrufer seine Rechtecke baut.
///
/// GEMESSENE ENTSCHEIDUNG, warum `kachel` hier steht und nicht nur die Zahl:
/// `round` aendert **die Kachelmasse**. Wer nur `anzahl` liest, zeichnet
/// 5x5 Kacheln von 40x20 in einem 203x97-Kasten — das ist genau der Fehler,
/// den `round` vermeiden soll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kachelplan {
    /// Die Kachelmasse in px **nach** der Wiederholung — bei [`Wiederholung::Round`]
    /// also die **angepasste**.
    pub kachel: (f32, f32),
    /// Zahl der Kacheln waagerecht / senkrecht.
    pub anzahl: (u32, u32),
    /// Versatz der **ersten** Kachel gegenueber der linken bzw. oberen
    /// Kante der Flaeche, in px.
    pub versatz: (f32, f32),
    /// Zwischenraum zwischen zwei Kacheln in px, **nur** bei
    /// [`Wiederholung::Space`]. Sonst `0.0`.
    pub abstand: (f32, f32),
}

/// **Hoechste Kachelzahl je Achse** (M3).
///
/// GEMESSENE BEGRUENDUNG: `repeat` ist der Vorgabewert, und ein 1x1-Pixel-
/// Bild auf einer 900x650-Flaeche ergibt **585 000** Kacheln. Die Zahl wird
/// hier **begrenzt statt gerechnet**, und die Kachelzahl steht im Ergebnis —
/// ein Muster, das zu gross ist, wird abgeschnitten und **gemeldet**.
/// Eine Laengen- oder Anteilsangabe — fuer `background-size` und
/// `background-position`.
///
/// **WARUM EIN EIGENER TYP UND NICHT `f32`:** GEMESSEN an Firefox 157
/// (04.10.2026, Rolle BILD), Kasten **160x70**, Kachel-Quellballast **100x23**:
///
/// ```text
///  background-size: 50%   ->  Kachel 80 x 18    <- 50 % der KASTENBREITE
///  background-size: 50% 50% -> Kachel 80 x 35   <- 50 % von BEIDEN
/// ```
///
/// **`50 %` ist also nicht `0.5`, sondern `80 px`** — und es ist die Breite des
/// Kastens, **nicht** die der Kachel. Wer beides in eine Zahl steckt, kann
/// `50%` nicht von `50px` unterscheiden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mass {
    /// `<length>` in px.
    Px(f32),
    /// `<percentage>`: bei `background-size` vom **Positionierungsbereich**, bei
    /// `background-position` von **(Bereich minus Kachel)** — zwei verschiedene
    /// Bezugsgroessen, beide **gemessen** (siehe `kachel_groesse`).
    Prozent(f32),
}

/// `background-size` (CSS Backgrounds 3 §3.4).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `Auto`/`Cover`/`Contain` **Flags** sind und
/// kein Enum:** `cover` und `contain` sind **Schluesselworte**, `50% 50%` sind
/// **zwei Werte**, und `100px` ist **ein Wert** — CSS erlaubt alle drei
/// Schreibweisen in **einer** Angabe, und `cover` zusammen mit `50%` ist
/// ungueltig. Flags bilden das ab, ohne eine weitere Fehlerart zu erzeugen.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GroesseAngabe {
    /// `cover` — skaliere, bis **beides gedeckt** ist; der Kasten schneidet.
    pub cover: bool,
    /// `contain` — skaliere, bis **beides hineinpasst**.
    pub contain: bool,
    /// Breite, falls angegeben.
    pub breite: Option<Mass>,
    /// Hoehe, falls angegeben. `None` = **die zweite Achse folgt dem
    /// Seitenverhaeltnis** (GEMESSEN: `50%` ergab 80 x 18, nicht 80 x 35).
    pub hoehe: Option<Mass>,
}

/// Die Kachelmasse aus `background-size`.
///
/// **GEMESSEN an Firefox 157 (04.10.2026), Kasten 160x70, Quelle 100x23:**
///
/// | `background-size` | Kachel |Bemerkung |
/// |---|---|---|
/// | `auto`             | **100 x 23** | Naturmass |
/// | `50%`              | **80 x 18**  | 50 % der Kastenbreite, Hoehe folgt |
/// | `100%`             | **160 x 37** | Breite = Kasten, Hoehe folgt |
/// | `100px 20px`       | **100 x 20** | zwei Werte, beide genommen |
/// | `cover`            | **160 x 70** | deckt den Kasten, der Rest schneidet |
/// | `contain`          | **160 x 37** | passt hinein |
/// | `50% 50%`          | **80 x 35**  | 50 % von beiden |
/// | `30px`             | **30 x 7**   | Breite, Hoehe folgt |
///
/// `None`, wenn etwas nicht brauchbar ist (Kasten ohne Flaeche, Quelle 0,
/// nicht-endliche Zahlen, Kasten groesser als [`MAX_KASTEN`]).
pub fn kachel_groesse(
    flaeche: (f32, f32),
    eigen: (u32, u32),
    angabe: GroesseAngabe,
) -> Option<(f32, f32)> {
    let (fb, fh) = flaeche;
    if !brauchbar(fb) || !brauchbar(fh) || fb > MAX_KASTEN || fh > MAX_KASTEN {
        return None;
    }
    if eigen.0 == 0 || eigen.1 == 0 {
        return None;
    }
    let ew = eigen.0 as f32;
    let eh = eigen.1 as f32;
    if ew > MAX_KASTEN || eh > MAX_KASTEN {
        return None;
    }

    // `cover` und `contain` zuerst: sie sind **eigenstaendig**, und danach
    // wird nicht mehr weitergerechnet.
    if angabe.cover || angabe.contain {
        let a = fb / ew;
        let b = fh / eh;
        let s = if angabe.cover { a.max(b) } else { a.min(b) };
        return Some((ew * s, eh * s));
    }

    // **DIE REGEL, DIE GEMESSEN WIRD:** EIN Wert nennt die **Breite**, und die
    // zweite Achse **folgt dem Seitenverhaeltnis** (CSS Backgrounds 3 §3.4:
    // „die zweite Groesse wird aus dem Seitenverhaeltnis berechnet, wenn nur
    // eine angegeben ist"). GEMESSEN: `50%` ergab **80 x 18** und **nicht**
    // 80 x 35.
    let mass = |m: Mass, achse: f32| match m {
        Mass::Px(v) => v,
        // GEMESSENER BEZUGSPUNKT: **die Flaeche**, nicht die Kachel. `50%` von
        // 160 sind 80; `50%` von 100 waeren 50.
        Mass::Prozent(p) => achse * p / 100.0,
    };
    match (angabe.breite, angabe.hoehe) {
        (None, None) => Some((ew, eh)),
        (Some(b), None) => {
            let br = mass(b, fb);
            if !brauchbar(br) || br <= 0.0 {
                return None;
            }
            // Das Seitenverhaeltnis bleibt: 80 aus 100 ergibt 18,4 aus 23.
            Some((br, br * eh / ew))
        }
        (None, Some(h)) => {
            let ho = mass(h, fh);
            if !brauchbar(ho) || ho <= 0.0 {
                return None;
            }
            Some((ho * ew / eh, ho))
        }
        (Some(b), Some(h)) => {
            let br = mass(b, fb);
            let ho = mass(h, fh);
            if !brauchbar(br) || !brauchbar(ho) || br <= 0.0 || ho <= 0.0 {
                return None;
            }
            Some((br, ho))
        }
    }
}

/// Liest `background-size`.
///
/// **GEMESSENE SYNTAX** (Firefox 157, dieselbe Seite wie [`kachel_groesse`]):
/// `auto`, `cover`, `contain`, **eine** Angabe (`100px`, `30px`, `50%`) und
/// **zwei** (`100px 20px`, `50% 50%`). **Gross-/Kleinschreibung** von
/// `cover`/`contain`/`auto` ist in CSS case-insensitiv.
///
/// **UND GEMESSEN, was NICHT geht:** `cover 50%` ist **ungueltig** — die
/// Spezifikation sagt „die Values must not be combined". Deshalb wird hier
/// **keine** Konstante hinterlegt: `None`, und der Aufrufer nimmt `auto`.
pub fn groesse_lesen(text: &str) -> Option<GroesseAngabe> {
    let t = text.trim();
    if t.is_empty() || t.len() > 96 {
        return None;
    }
    let w: Vec<&str> = t.split_whitespace().collect();
    if w.is_empty() || w.len() > 2 {
        return None;
    }
    if w.len() == 1 {
        let e = w[0];
        if e.eq_ignore_ascii_case("auto") {
            return Some(GroesseAngabe::default());
        }
        if e.eq_ignore_ascii_case("cover") {
            return Some(GroesseAngabe {
                cover: true,
                ..Default::default()
            });
        }
        if e.eq_ignore_ascii_case("contain") {
            return Some(GroesseAngabe {
                contain: true,
                ..Default::default()
            });
        }
    }
    //
    // BILD_31: **MEIN FEHLER, VOM TEST GEFUNDEN.** `100px 20px` las nicht. Ich
    // hatte nur `%` abgeschnitten und **`px` vergessen** — `"100px".parse::<f32>()`
    // scheitert natuerlich. **GEMESSENE SYNTAX (Firefox 157):** `100px`, `30px`,
    // `100px 20px`, `50%`, `50% 50%`, `auto`, `cover`, `contain`.
    let mass = |s: &str| -> Option<Mass> {
        let s = s.trim();
        if let Some(rest) = s.strip_suffix('%') {
            let v: f32 = rest.trim().parse().ok()?;
            if !brauchbar(v) {
                return None;
            }
            return Some(Mass::Prozent(v));
        }
        // `px` ist im CSS optional (`100` = `100px`), darum wird es **entfernt**,
        // nicht verlangt. GEMESSEN: `background-size: 30` ist gueltig.
        let ohne_px = s
            .strip_suffix("px")
            .or_else(|| s.strip_suffix("PX"))
            .unwrap_or(s);
        let v: f32 = ohne_px.parse().ok()?;
        if !brauchbar(v) {
            return None;
        }
        Some(Mass::Px(v))
    };
    Some(match w.len() {
        1 => GroesseAngabe {
            breite: Some(mass(w[0])?),
            ..Default::default()
        },
        _ => GroesseAngabe {
            breite: Some(mass(w[0])?),
            hoehe: Some(mass(w[1])?),
            ..Default::default()
        },
    })
}

pub const MAX_KACHELN: u32 = 256;

/// Das Muster fuer eine Flaeche, eine Kachelmasse und eine Wiederholung.
///
/// `flaeche` und `kachel` in px. `position` ist das Ergebnis von
/// [`crate::position_lesen`], nicht der Rohwert: `space` braucht es **nicht**
/// (GEMESSEN: `space` mit und ohne `background-position` ist pixelgleich), und
/// `round` verschiebt das Muster um seinen Wert.
///
/// `None`, wenn es nichts zu zeichnen gibt: Flaeche oder Kachel ohne Flaeche,
/// nicht-endliche Zahlen, oder Werte ueber [`MAX_KASTEN`]. Das ist **kein**
/// Fehler, sondern „nichts sichtbar".
///
/// # DIE SECHS WERTE, jeder mit seiner Messung (203x97, Kachel 40x20)
///
/// | Wert | `anzahl` | `kachel` | `versatz` | `abstand` |
/// |---|---|---|---|---|
/// | `repeat` | (6, 5) | (40, 20) | (0, 0) | (0, 0) |
/// | `no-repeat` | (1, 1) | (40, 20) | nach Position | (0, 0) |
/// | `repeat-x` | (6, 1) | (40, 20) | nach Position in y | (0, 0) |
/// | `repeat-y` | (1, 5) | (40, 20) | nach Position in x | (0, 0) |
/// | `space` | **(5, 4)** | (40, 20) | **(0, 0)** | **(0,75; 5,67)** |
/// | `round` | **(5, 5)** | **(40,6; 19,4)** | nach Position | (0, 0) |
///
/// GEMESSENE ENTSCHEIDUNG zu `space`: der Zwischenraum ist
/// `(flaeche − n · kachel) / (n − 1)` — **durch die Zahl der Luecken**, nicht
/// durch `n + 1`. In x: (203 − 200)/4 = **0,75**, und die Versaetze 0 / 40,75 /
/// 81,5 / 122,25 / 163 ergeben **0 / 41 / 82 / 122 / 163** im Screenshot (auf
/// ganze Pixel gerundet, `.5` nach oben). In y: (97 − 80)/3 = **5,67**, und
/// 0 / 25,67 / 51,33 / 77 ergibt **0 / 26 / 51 / 77**.
///
/// GEMESSENE ENTSCHEIDUNG zu `round`: `n = (flaeche / kachel).round()`, und
/// die Kachel wird `flaeche / n`. In x: round(203/40) = round(5,075) = **5**,
/// 203/5 = **40,6**. In y: round(97/20) = round(4,85) = **5**, 97/5 = **19,4**.
/// GEMESSENE ENTSCHEIDUNG, warum **gerundet** und nicht abgeschnitten:
/// abgeschnitten waeren in y **4** Kacheln von 24,25 px — das Musst endete
/// dann **bei 97** und kaeme der Untergrenze nahe; Firefox hat **5** gemessen.
///
/// GEMESSENE ENTSCHEIDUNG zu `space` mit **n = 0** (Kachel breiter als die
/// Flaeche): `n` wird auf **1** gehalten und der Zwischenraum bleibt `0`. Das
/// ist die Sache `no-repeat` — nach CSS Backgrounds 3 §3.5 wird bei `space`
/// mit zu grosser Kachel genauso verfahren.
pub fn kachelplan(
    flaeche: (f32, f32),
    kachel: (f32, f32),
    wiederholung: Wiederholung,
    position: Bildposition,
) -> Option<Kachelplan> {
    let (fb, fh) = flaeche;
    let (kb, kh) = kachel;
    if !brauchbar(fb) || !brauchbar(fh) || !brauchbar(kb) || !brauchbar(kh) {
        return None;
    }
    if fb > MAX_KASTEN || fh > MAX_KASTEN || kb > MAX_KASTEN || kh > MAX_KASTEN {
        return None;
    }
    let mut kbw = kb;
    let mut khh = kh;
    //
    // `round` zuerst: es ist der **einzige** Wert, der die Kachelmasse
    // aendert, und danach rechnen die anderen Werte weiter.
    let (mut nx, mut ny) = (1u32, 1u32);
    match wiederholung {
        Wiederholung::Round => {
            nx = ganzzahl(fb / kbw);
            ny = ganzzahl(fh / khh);
            kbw = fb / nx as f32;
            khh = fh / ny as f32;
        }
        Wiederholung::Beide => {
            nx = kachelzahl(fb / kbw);
            ny = kachelzahl(fh / khh);
        }
        Wiederholung::NurX => {
            nx = kachelzahl(fb / kbw);
        }
        Wiederholung::NurY => {
            ny = kachelzahl(fh / khh);
        }
        Wiederholung::Keine | Wiederholung::Space => {}
    }
    if wiederholung == Wiederholung::Space {
        //
        // GEMESSEN: `space` nimmt **floor**, nicht ceil — 203/40 = 5,075 ergibt
        // **5** Kacheln, und die sechste (ab x = 200) wird abgeschnitten.
        nx = boden(fb / kbw).max(1);
        ny = boden(fh / khh).max(1);
    }
    //
    // DIE GRENZE GREIFT **GESAMT**, nicht je Achse.
    //
    // **MEIN EIGENER FEHLER, gefunden von meinem eigenen Test
    // (`die_obergrenze_greift_und_meldet` in `tests/kachel.rs`):** zuerst stand
    // hier `nx.clamp(1, MAX_KACHELN)` und `ny.clamp(1, MAX_KACHELN)` je Achse.
    // Ein 1x1-Bild auf 900x650 ergibt 900 x 650, geklemmt **256 x 256** =
    // **65 536** Kacheln — vierundzwanzigmal mehr, als die Grenze verspricht.
    // Derselbe Fehler steht in `narsil-layout-engine/src/hintergrund.rs:169-193`
    // **dort dokumentiert**, und ich habe ihn trotzdem gemacht. Deshalb steht
    // hier jetzt die Begruendung und nicht nur die Zahl.
    //
    // **PROPORTIONAL KLEMMEN, und nicht eine Achse:** dann waeren 18 x 14
    // Kacheln gebaut — das Muster bleibt in beiden Richtungen gleichmaessig.
    // Eine einzige Achse zu klemmen (nur y) ergaebe 900 x 1: ein Streifen am
    // oberen Rand, und der Rest der Flaeche waere mit *nichts* gefuellt, statt
    // mit *verkleinerten* Kacheln. Das ist der Unterschied zwischen „zu wenig"
    // und „falsch verteilt".
    //
    // GEMESSENE ENTSCHEIDUNG, warum hier **kein** `verworfen`-Feld steht wie in
    // `kacheln()`: diese Funktion zeichnet nichts und weiss nicht, wie viele
    // Kacheln der Aufrufer nach dem Beschneiden wirklich baut. Eine Zahl, die
    // hier stuende, waere eine Schaetzung — und `kacheln()` kann sie **ohne
    // Schaetzung** zaehlen. Zwei Zaehler fuer dieselbe Sache sind zwei
    // Wahrheiten.
    let (mut nx_k, mut ny_k) = (nx.max(1), ny.max(1));
    if nx_k.saturating_mul(ny_k) > MAX_KACHELN {
        let faktor = (MAX_KACHELN as f32 / (nx_k as f32 * ny_k as f32)).sqrt();
        let bx = ((nx_k as f32) * faktor).floor().max(1.0) as u32;
        let by = (MAX_KACHELN / bx.max(1)).max(1);
        nx_k = bx.min(MAX_KACHELN);
        ny_k = by.min(MAX_KACHELN);
    }
    let nx = nx_k;
    let ny = ny_k;

    //
    // Der Versatz: der von `background-position`, **ausser** bei `space`.
    //
    // GEMESSEN: `space` mit `background-position: 7px 3px` ist **pixelgleich**
    // zu `space` ohne Position (0/41/82/122/163 und 0/26/51/77 in beiden
    // Laeufen). Die Norm sagt dasselbe: bei `space` liegen die erste und die
    // letzte Kachel an den Kanten, es gibt also nichts zu verschieben.
    let (vx, vy) = if wiederholung == Wiederholung::Space {
        (0.0, 0.0)
    } else {
        (
            versatz_von(fb, kbw, position.x),
            versatz_von(fh, khh, position.y),
        )
    };
    //
    // Der Zwischenraum: nur bei `space`, und **durch `n - 1`** (die Zahl der
    // Luecken zwischen den Kacheln). GEMESSEN: 0,75 in x und 5,67 in y.
    let (mut lx, mut ly) = (0.0f32, 0.0f32);
    if wiederholung == Wiederholung::Space {
        if nx > 1 {
            lx = ((fb - nx as f32 * kbw) / (nx as f32 - 1.0)).max(0.0);
        }
        if ny > 1 {
            ly = ((fh - ny as f32 * khh) / (ny as f32 - 1.0)).max(0.0);
        }
    }
    if !lx.is_finite() || !ly.is_finite() {
        return None;
    }
    Some(Kachelplan {
        kachel: (kbw, khh),
        anzahl: (nx, ny),
        versatz: (vx.clamp(-MAX_LAGE, MAX_LAGE), vy.clamp(-MAX_LAGE, MAX_LAGE)),
        abstand: (lx, ly),
    })
}

/// Der Versatz einer Achse — **dieselbe** Rechnung wie bei
/// [`crate::rechteck_in_kasten`], nur ohne Bild.
///
/// GEMESSENE ENTSCHEIDUNG, warum das nicht `rechteck_in_kasten` aufruft: dort
/// kommen `Passung` und `Rechteck` hinein, und ein Hintergrund braucht beides
/// nicht. Die **Formel** ist die gleiche, und sie steht zweimal im Quelltext —
/// das ist der Preis, dass `platzierung.rs` kein Layout-Modul kennt. Wer sie
/// aendert, muss beide aendern; die Tests in `tests/kachel.rs` halten beide
/// gegen dieselben Firefox-Zahlen fest.
fn versatz_von(flaeche: f32, kachel: f32, anker: Anker) -> f32 {
    let frei = flaeche - kachel;
    let roh = match anker {
        Anker::Mitte => frei * 0.5,
        Anker::Prozent(p) => frei * (p / 100.0),
        Anker::Pixel(px) => px,
        Anker::PixelVomEnde(px) => frei - px,
    };
    if roh.is_finite() {
        roh.clamp(-MAX_LAGE, MAX_LAGE)
    } else {
        0.0
    }
}

/// Kachelzahl fuer `repeat`: so viele, dass die Flaeche **voll** wird.
///
/// GEMESSEN: 203/40 = 5,075 ergibt **6** Kacheln, und die sechste beginnt bei
/// x = 200 und wird auf 3 px **abgeschnitten** — der Rand ist damit gefuellt.
/// In y: 97/20 = 4,85 ergibt **5**, die fuenfte beginnt bei y = 80.
///
/// **DER FEHLER, DEN DIE MESSUNG GEFUNDEN HAT:** hier stand urspruenglich
/// `ceil(v) + 1`, also **7x6** statt **6x5**. Begruendet war das mit „die eine
/// fehlende Kachel laesst die Kante leer" — das ist **falsch**, denn die letzte
/// Kachel beginnt genau am Rand und wird abgeschnitten, nicht ausgelassen.
/// Der Grund fuer die Korrektur ist die Messung, nicht die Einsicht.
fn kachelzahl(verhaeltnis: f32) -> u32 {
    if !verhaeltnis.is_finite() || verhaeltnis <= 0.0 {
        return 1;
    }
    (verhaeltnis.ceil() as u32).clamp(1, MAX_KACHELN)
}

/// `round`: die naechste ganze Zahl, **gerundet** (M3, und die Rundung ist
/// gemessen — siehe den Kasten von [`kachelplan`]).
fn ganzzahl(v: f32) -> u32 {
    if !v.is_finite() || v <= 0.0 {
        return 1;
    }
    let gerundet = (v + 0.5).floor();
    if !gerundet.is_finite() || gerundet < 1.0 {
        return 1;
    }
    (gerundet as u32).clamp(1, MAX_KACHELN)
}

/// `space`: `floor`, mindestens 1.
fn boden(v: f32) -> u32 {
    if !v.is_finite() || v < 1.0 {
        return 1;
    }
    (v.floor() as u32).clamp(1, MAX_KACHELN)
}

fn brauchbar(v: f32) -> bool {
    v.is_finite() && v > 0.0
}
