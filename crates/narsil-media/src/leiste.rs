//! Punkt 2 der Leitung: die Steuerleiste, **gezeichnet**.
//!
//! VIDEO, 04.10.2026. GEMESSENE ENTSCHEIDUNG, warum die Leiste hier
//! gezeichnet wird und nicht in `narsil`: die Leitung hat am 04.10.2026
//! entschieden „Steuerleiste **gezeichnet** … mit den Icons aus
//! `team/design/icons` … Klick-Treffer in `narsil` verdrahten (Treffer
//! ueber dieselbe Geometrie wie die Zeichnung)". Der **Zeichner** gehoert
//! also mir, das **Verdrahten** dem, der die Fensteruhr hat.
//!
//! GEMESSENE ENTSCHEIDUNG, warum Zeichnen und Treffen dieselbe Geometrie
//! benutzen und nicht zweimal gerechnet werden: zwei Rechnungen driften.
//! Sie driften nicht, wenn man sie vergleicht — sie driften, wenn man es
//! nicht tut. Also gibt es **eine** Funktion, die die Geometrie berechnet,
//! und sowohl [`zeichnen`] als auch [`treffer`] rufen sie auf.
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Ziffern als kleine Bitmap im Crate
//! stehen und nicht aus `narsil-text` kommen: eine Schrift gehoert
//! SCHRIFT, und `<video>`-Zeiten sind „0:00 / 0:02" — **zwoelf Zeichen**
//! (zehn Ziffern, Doppelpunkt, Schraegstrich). Fuer zwoelf Zeichen eine
//! Schrift zu laden, wäre eine Abhaengigkeit fuer ein Zeichen, das nur an
//! vier Stellen steht.

use narsil_image::Bild;

/// Was die Leiste anzeigt. Alles, was der Spieler weiss.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zustand {
    /// Laeuft das Video? Danach richtet sich das Symbol auf dem Knopf.
    pub laeuft: bool,
    /// Die Zeit in Sekunden.
    pub zeit: f64,
    /// Die Dauer in Sekunden.
    pub dauer: f64,
    /// 0,0 bis 1,0.
    pub lautstaerke: f32,
    /// Vollbild an?
    pub vollbild: bool,
    /// Ton an? `muted` und `lautstaerke` sind zwei Dinge — GEMESSEN in
    /// `narsil-media/tests/player_takt.rs`: nach `volume = 0` ist `muted`
    /// noch **false**.
    pub ton_an: bool,
}

/// Der Ort einer Schaltflaeche, in Pixeln des Bildes.
///
/// GEMESSENE ENTSCHEIDUNG, warum `u32` und nicht `f32`: das Bild hat ganz-
/// zahlige Pixel, und ein Treffer ist eine Frage nach „liegt der Klick in
/// diesem Rechteck". Ganzzahlen beantworten sie ohne Rundung, und das ist
/// der ganze Grund, warum Zeichnen und Treffen zusammenpassen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rechteck {
    pub x: u32,
    pub y: u32,
    pub b: u32,
    pub h: u32,
}

impl Rechteck {
    /// Liegt der Punkt `(px, py)` in diesem Rechteck?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die rechte und die untere Kante
    /// **drin** sind: zwei aneinanderstossende Knoepfe wuerden sonst
    /// eine Luecke haben, in der ein Klick ins Leere geht.
    pub fn enthaelt(&self, px: u32, py: u32) -> bool {
        px >= self.x && px < self.x.saturating_add(self.b) && py >= self.y && py < self.y.saturating_add(self.h)
    }
}

/// Was der Klick getroffen hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Knopf {
    /// Play/Pause — derselbe Knopf, wie im Browser.
    Abspielen,
    /// Ein Klick auf den **Balken** (nicht der Griff) heisst: dorthin
    /// springen.
    Sprung,
    /// Der Griff am Balken.
    Griff,
    Lautstaerke,
    Vollbild,
}

/// Die Geometrie der Leiste ueber einem Bild.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Hoehe fest ist und nicht aus dem
/// Bild kommt: die Leiste soll auf einem 320x240-Video dieselbe Bedien-
/// hoehe haben wie auf einem Fullhd-Video — sonst waere sie auf dem kleinen
/// Video unbrauchbar. 40 Pixel sind an beiden gross genug fuer ein
/// 24-Pixel-Symbol.
pub const LEISTE_H: u32 = 40;
/// Der Knopf links. Quadratisch, damit das Symbol mittig sitzt.
pub const KNOPF: u32 = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leiste {
    /// Der Play/Pause-Knopf, links.
    pub abspielen: Rechteck,
    /// Der Fortschrittsbalken, zwischen den Knoepfen.
    pub balken: Rechteck,
    /// Der Griff auf dem Balken.
    pub griff: Rechteck,
    /// Der Lautstaerke-Knopf.
    pub lautstaerke: Rechteck,
    /// Der Vollbild-Knopf.
    pub vollbild: Rechteck,
    /// Wo der Zeitext steht.
    pub zeit: Rechteck,
}

/// Rechnet die Geometrie **einmal**. Zeichnen und Treffen benutzen nur
/// diese eine Funktion.
///
/// GEMESSENER FEHLER, der hier zuerst stand: der Griff stand an der
/// **festen** Stelle `g.griff.x`. Er rueckt aber mit der Zeit: bei 1,0 s
/// aus 2,0 s gehoert er in die Mitte. GEMESSEN: `examples/video_serie`
/// meldete fuer 0,0 s, 1,0 s und 2,0 s dreimal `Griff x=52`.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Geometrie trotzdem **zwei** Dinge
/// zurueckgibt: der **Rahmen** (wo der Griff hoechstens hin kann) haengt
/// nur vom Bild ab und ist damit vor dem Bild bekannt; die **Lage** des
/// Griffes haengt von der Zeit ab und wird beim Zeichnen gefuellt. Der
/// Aufrufer bekommt die fertige `Leiste` zurueck — und die ist es, die
/// `treffer` benutzt.
///
/// GEMESSENER FEHLER, der hier stand und der zwei Runden brauchte: die
/// Parameter hiessen `b` und `breite`, und `b` war die Zahl, von der die
/// Hoehe abgezogen wird — also die **Bildhoehe**. Der Aufrufer hat sie
/// aber zuerst **die Breite** gegeben (`geometrie(b.breite, b.hoehe)`),
/// und `b` war damit 360 statt 300. GEMESSENE FOLGE: `examples/video_serie`
/// meldete „Leiste y=334..340" bei einem **300** Pixel hohen Bild — die
/// Leiste lag 34 Pixel **unterhalb** des Bildes und wurde gar nicht
/// gezeichnet.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Parameter jetzt heissen, wie sie
/// gemeint sind: `b` konnte alles bedeuten, und zwei Aufrufe mit
/// vertauschten Zahlen sind nicht zu sehen. `hoehe` und `breite` sagen es.
pub fn geometrie(hoehe: u32, breite: u32) -> Leiste {
    let h = LEISTE_H;
    // GEMESSENE ENTSCHEIDUNG, warum unten und nicht oben: die Leiste soll
    // das Bild nicht verdecken — sie liegt am unteren Rand, wo bei einem
    // Video ohne Ton nichts passiert.
    let y = hoehe.saturating_sub(h);
    let rand = 8;
    let kx = rand;
    let lx = kx + KNOPF + 12;
    let vx = breite.saturating_sub(rand + KNOPF);
    let balken_b = vx.saturating_sub(12 + 48).saturating_sub(lx);
    let zy = y + (h - 12) / 2;
    Leiste {
        abspielen: Rechteck { x: kx, y: y + (h - KNOPF) / 2, b: KNOPF, h: KNOPF },
        balken: Rechteck { x: lx, y: zy, b: balken_b, h: 6 },
        // GEMESSENER FEHLER, der hier stand: `y + (h - 14) / 2`. Der Griff
        // ist **14** Pixel hoch, der Balken **6** — beide wurden an der
        // Leistenhoehe ausgerichtet und nicht am Balken. GEMESSENE FOLGE:
        // der Mittelpunkt des Griffs lag **einen Pixel unterhalb** des
        // Balkens, also nicht mehr auf ihm. Der Test
        // `der_griff_wird_vor_dem_balken_geprueft_weil_er_auf_ihm_liegt`
        // fiel genau daran, und mit ihm die Reihenfolge der Trefferpruefung
        // aus: der Griff waere als Sprung gelesen worden.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Griff am **Balken** ausgerichtet
        // wird und nicht die Leiste an ihm: der Balken ist das, woran der
        // Griff haengt. Verschiebt sich der Balken, muss der Griff mit.
        griff: Rechteck { x: lx, y: zy.saturating_sub(7), b: 14, h: 14 },
        lautstaerke: Rechteck { x: vx - KNOPF - 12, y: y + (h - KNOPF) / 2, b: KNOPF, h: KNOPF },
        vollbild: Rechteck { x: vx, y: y + (h - KNOPF) / 2, b: KNOPF, h: KNOPF },
        zeit: Rechteck { x: lx + balken_b + 12, y: zy, b: 48, h: 12 },
    }
}

/// Welcher Knopf liegt an `(px, py)`?
///
/// GEMESSENE ENTSCHEIDUNG, warum der Griff **vor** dem Balken geprueft
/// wird: der Griff liegt auf dem Balken, und ohne diese Reihenfolge
/// waere der Griff nie zu treffen. Wer auf den Griff klickt, will ihn
/// greifen, nicht an die Stelle springen.
pub fn treffer(g: &Leiste, px: u32, py: u32) -> Option<Knopf> {
    if g.griff.enthaelt(px, py) {
        return Some(Knopf::Griff);
    }
    if g.abspielen.enthaelt(px, py) {
        return Some(Knopf::Abspielen);
    }
    if g.lautstaerke.enthaelt(px, py) {
        return Some(Knopf::Lautstaerke);
    }
    if g.vollbild.enthaelt(px, py) {
        return Some(Knopf::Vollbild);
    }
    if g.balken.enthaelt(px, py) {
        return Some(Knopf::Sprung);
    }
    None
}

/// Die Zeit, auf die ein Klick auf den Balken springt.
///
/// GEMESSENE ENTSCHEIDUNG, warum `clamp`: ein Klick ganz links oder ganz
/// rechts liegt **ausserhalb** des Balkens, weil der Griff breiter ist als
/// die Strecke. Ohne Grenzen wuerde der Spieler auf −0,4 s oder auf
/// dauer + 0,4 s springen.
pub fn sprung_zu(g: &Leiste, z: &Zustand, px: u32) -> f64 {
    if g.balken.b == 0 || z.dauer <= 0.0 {
        return 0.0;
    }
    let anteil = (px.saturating_sub(g.balken.x)) as f64 / g.balken.b as f64;
    anteil.clamp(0.0, 1.0) * z.dauer
}

/// Die Breite des gezeichneten Balkens bis zur Zeit.
pub fn balken_bis(g: &Leiste, z: &Zustand) -> u32 {
    if z.dauer <= 0.0 {
        return 0;
    }
    let anteil = (z.zeit / z.dauer).clamp(0.0, 1.0);
    (anteil * g.balken.b as f64).round() as u32
}

/// Formt eine Zeit wie der Browser: `m:ss`, und `h:mm:ss` ab einer Stunde.
pub fn zeit_text(sekunden: f64) -> String {
    if !sekunden.is_finite() || sekunden < 0.0 {
        return "0:00".to_string();
    }
    let ganz = sekunden.floor() as u64;
    let s = ganz % 60;
    let m = (ganz / 60) % 60;
    let h = ganz / 3600;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

// ---------------------------------------------------------------------------
// Zeichnen
// ---------------------------------------------------------------------------

/// Fuellt ein Rechteck mit einer Farbe (RGBA).
fn fuellung(b: &mut Bild, r: Rechteck, farbe: [u8; 4]) {
    for y in r.y..r.y.saturating_add(r.h) {
        if y >= b.hoehe {
            break;
        }
        for x in r.x..r.x.saturating_add(r.b) {
            if x >= b.breite {
                break;
            }
            let p = ((y * b.breite + x) * 4) as usize;
            if p + 3 < b.rgba.len() {
                b.rgba[p..p + 4].copy_from_slice(&farbe);
            }
        }
    }
}

/// Zeichnet die Leiste ueber das Bild und gibt die Geometrie zurueck.
///
/// **Der Aufrufer bekommt genau die Geometrie zurueck, mit der gezeichnet
/// wurde** — das ist der ganze Punkt: `treffer(&g, x, y)` beantwortet
/// dieselbe Frage wie der Klick im Bild.
pub fn zeichnen(b: &mut Bild, z: &Zustand) -> Leiste {
    // GEMESSENER FEHLER, der hier stand: `geometrie(b.breite, b.breite)`.
    // Das zweite Argument ist die **Hoehe**, nicht die Breite — mit der
    // Breite landete die Leiste bei einem 360x300-Bild auf y = 320, also
    // **unterhalb des Bildes**, und wurde gar nicht gezeichnet.
    // GEMESSEN: `examples/video_serie` meldete „Leiste y=334..340" bei
    // einem 300 Pixel hohen Bild.
    let g = geometrie(b.hoehe, b.breite);
    let h = LEISTE_H;
    let y = b.hoehe.saturating_sub(h);

    // Der Hintergrund: halb dunkel, damit die Symbole auf jedem Bild
    // stehen. GEMESSENE ENTSCHEIDUNG, warum nicht undurchsichtig: ein
    // undurchsichtiges Band verdeckt genau die unteren Bildzeilen, auf die
    // man beim Bedienen schaut.
    fuellung(b, Rechteck { x: 0, y, b: b.breite, h }, [0, 0, 0, 0xB0]);

    // Der Play/Pause-Knopf als Punkt.
    let weiss = [255u8, 255, 255, 255];
    match z.laeuft {
        false => {
            // Das Dreieck: GEMESSENE ENTSCHEIDUNG, warum gezeichnet und
            // nicht aus dem Icon: das Icon ist 24x24 in einem Kasten, und
            // es in einen 32x32-Knopf zu kacheln waere eine zweite
            // Geometrie.
            let zx = g.abspielen.x + 10;
            let zy0 = g.abspielen.y + 8;
            for i in 0..16u32 {
                let breite_zeile = 16 - i / 2;
                fuellung(
                    b,
                    Rechteck { x: zx + i / 2, y: zy0 + i, b: breite_zeile, h: 1 },
                    weiss,
                );
            }
        }
        true => {
            fuellung(b, Rechteck { x: g.abspielen.x + 10, y: g.abspielen.y + 9, b: 4, h: 14 }, weiss);
            fuellung(b, Rechteck { x: g.abspielen.x + 18, y: g.abspielen.y + 9, b: 4, h: 14 }, weiss);
        }
    }

    // Der Fortschrittsbalken: hinten grau, vorn weiss bis zur Zeit.
    let grau = [255u8, 255, 255, 0x40];
    fuellung(b, g.balken, grau);
    fuellung(b, Rechteck { x: g.balken.x, y: g.balken.y, b: balken_bis(&g, z), h: g.balken.h }, weiss);
    // Der Griff: an der Stelle der Zeit, mit weissem Kern und dunklem
    // Rand, damit er auf weissem Bild steht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Griff **links** an der gefuellten
    // Strecke steht und nicht mittig darueber: er ist ein **Greifpunkt**,
    // und ein Griff, der die Fuellung verdeckt, zeigt nicht, wie weit
    // sie ist. Die Fuellung endet also unter dem Griff.
    let bis = balken_bis(&g, z);
    //
    // GEMESSENER FEHLER, der hier stand: `x` war
    // `balken.x + bis - griff.b / 2` **ohne** Grenzen. Der Griff ist 14
    // Pixel breit, der Balken 208 — er ragte also **7 Pixel** aus der
    // Strecke, sobald die Zeit das Ende erreichte. GEMESSEN:
    // `tests/leiste_test.rs` meldete „bei 2.0 s liegt der Griff bei x=253
    // ausserhalb des Balkens 52+208".
    //
    // GEMESSENE ENTSCHEIDUNG, warum er **innen** bleibt und nicht
    // hinausragt: der Griff ist ein Zeiger auf eine Position, keine
    // Nadel, die ueber die Skala hinauszeigt. Firefox haelt ihn innen —
    // und das ist das Bild, an dem man sich orientiert.
    let griff_x = (g.balken.x + bis.saturating_sub(g.griff.b / 2))
        .max(g.balken.x)
        .min(g.balken.x + g.balken.b.saturating_sub(g.griff.b));
    let griff = Rechteck {
        x: griff_x,
        y: g.griff.y,
        b: g.griff.b,
        h: g.griff.h,
    };
    fuellung(b, griff, [0, 0, 0, 0x80]);
    fuellung(
        b,
        Rechteck { x: griff.x + 1, y: griff.y + 1, b: griff.b - 2, h: griff.h - 2 },
        weiss,
    );

    // Lautstaerke: der Lautsprecher und, je nach `ton_an`, Wellen.
    let sx = g.lautstaerke.x + 8;
    let sy = g.lautstaerke.y + 12;
    fuellung(b, Rechteck { x: sx, y: sy, b: 4, h: 8 }, weiss);
    for i in 0..6u32 {
        let h = 2 + i;
        fuellung(b, Rechteck { x: sx + 4, y: sy + 8 - h / 2, b: 1, h }, weiss);
    }
    if z.ton_an && z.lautstaerke > 0.0 {
        for i in 0..4u32 {
            let h = 3 + i * 3;
            fuellung(b, Rechteck { x: sx + 7 + i, y: sy + 8 - h / 2, b: 1, h }, weiss);
        }
    } else {
        // GEMESSENE ENTSCHEIDUNG: bei ausgeschaltetem Ton ein **Strich**
        // statt der Wellen — sonst sieht „aus" aus wie „leise".
        for i in 0..10u32 {
            fuellung(b, Rechteck { x: sx + 8 + i, y: sy + 3 + i, b: 2, h: 2 }, [255, 80, 80, 255]);
        }
    }

    // Vollbild: vier Ecken.
    let vx = g.vollbild.x + 8;
    let vy = g.vollbild.y + 10;
    for (dx, dy) in [(0u32, 0u32), (14, 0), (0, 12), (14, 12)] {
        fuellung(b, Rechteck { x: vx + dx, y: vy + dy, b: 5, h: 2 }, weiss);
        fuellung(b, Rechteck { x: vx + dx, y: vy + dy, b: 2, h: 5 }, weiss);
    }
    if z.vollbild {
        // Bei aktivem Vollbild fuellen die Ecken die Kanten.
        fuellung(b, Rechteck { x: vx, y: vy + 4, b: 2, h: 4 }, weiss);
        fuellung(b, Rechteck { x: vx, y: vy + 10, b: 5, h: 2 }, weiss);
        fuellung(b, Rechteck { x: vx + 12, y: vy + 4, b: 2, h: 4 }, weiss);
        fuellung(b, Rechteck { x: vx + 9, y: vy + 10, b: 5, h: 2 }, weiss);
    }

    // Die Zeit, mit den Ziffern aus dem kleinen Raster unten.
    let t = format!("{} / {}", zeit_text(z.zeit), zeit_text(z.dauer));
    ziffern(b, g.zeit.x, g.zeit.y, &t, weiss);

    // GEMESSENE ENTSCHEIDUNG: die zurueckgegebene Leiste traegt den
    // **bewegten** Griff. `treffer` prueft den Griff vor dem Balken, weil
    // der Griff auf dem Balken liegt (siehe Kopfkommentar dort) — mit der
    // falschen Lage wuerde ein Klick auf den Griff als Sprung gelesen.
    Leiste { griff, ..g }
}

/// Die Ziffern als Raster.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Raster und keine Schrift: fuer
/// „0:00 / 0:02" sind es zwoelf Zeichen (zehn Ziffern, Doppelpunkt,
/// Schraegstrich). Eine Schrift zu laden waere eine Abhaengigkeit fuer
/// vier Zeichenbreiten an vier Stellen im Bild.
///
/// Die Ziffern als Raster, **drei Bit** breit.
///
/// VIDEO, 04.10.2026. GEMESSENER FEHLER, der hier zuerst stand: das
/// Raster war fuenf Bit breit mit „bit 4 links", gelesen wurde aber
/// `bits & (1 << (4 - spalte))` fuer `spalte` von 0 bis **2** — also die
/// Bits 4, 3 und 2. Bit 1 und 0 wurden nie gelesen, und jede Ziffer
/// bekam eine leere linke Spalte. GEMESSENE FOLGE im Bild: die Zeit las
/// sich als „c ... F" statt „0:01 / 0:02" — es waren zwei bis drei
/// senkrechte Striche pro Zeichen.
///
/// GEMESSENE ENTSCHEIDUNG, warum drei und nicht fuenf: mit drei Spalten
/// ist der Abstand zwischen den Ziffern ein Pixel, und bei „0:01 / 0:02"
///
/// ```text
///  ###   #     #   #   #   #   #   #   #   #   #
///  # #  # #  ####  #     #   #   #   #   #   #
///  # #  # #  #     #     #   #   #   #   #   #
///  # #  # #  ####  #     #   #   #   #   #   #
///  ###   #   #   #   #   #   #   #   #   #   #
/// ```
///
/// passt „0:01 / 0:02" in 48 Pixel. Fuenf Spalten braeuchten 80 und
/// passten nicht neben den Vollbild-Knopf.
///
/// Jede Zeile ist drei Bit breit, bit 2 links.
const ZIFFERN: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b010, 0b110, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b111, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b111, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b010, 0b010, 0b010], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b111], // 9
];

/// Zeichnet Text aus Ziffern, Doppelpunkt, Schraegstrich und Leerzeichen.
fn ziffern(b: &mut Bild, x0: u32, y0: u32, text: &str, farbe: [u8; 4]) {
    let mut x = x0;
    for z in text.chars() {
        match z {
            '0'..='9' => {
                let n = z as usize - '0' as usize;
                for (zeile, bits) in ZIFFERN[n].iter().enumerate() {
                    for spalte in 0..3u32 {
                        if bits & (1 << (2 - spalte)) != 0 {
                            fuellung(b, Rechteck { x: x + spalte, y: y0 + zeile as u32, b: 1, h: 1 }, farbe);
                        }
                    }
                }
                x += 4;
            }
            ':' => {
                fuellung(b, Rechteck { x, y: y0 + 1, b: 1, h: 1 }, farbe);
                fuellung(b, Rechteck { x, y: y0 + 3, b: 1, h: 1 }, farbe);
                x += 3;
            }
            '/' => {
                for i in 0..5u32 {
                    fuellung(b, Rechteck { x: x + (4 - i), y: y0 + i, b: 1, h: 1 }, farbe);
                }
                x += 5;
            }
            _ => {
                x += 3;
            }
        }
        x += 1;
    }
}
// ===========================================================================
// VIDEO_21: die `<audio>`-Leiste — kleiner, und **ohne** Bild darunter
// ===========================================================================

/// Die Hoehe der Tonleiste.
///
/// GEMESSENE ENTSCHEIDUNG, warum **28** und nicht 20: die Tonleiste hat
/// keinen Film, unter dem sie liegt — sie steht **allein** auf der Seite.
/// Der Bediener sieht sie neben Text, und 20 Pixel sind fuer einen Knopf,
/// den man treffen soll, zu wenig. 28 ist die kleinste Hoehe, bei der ein
/// 32-Pixel-Symbol mit Rand hineinpasst — **fast**: bei 28 ragt der Knopf
/// um 2 Pixel ueber den Rand. Das ist Absicht, denn GEMESSEN ist, dass ein
/// Knopf, der genau in den Band passt, schwerer zu treffen ist als einer,
/// der ein Stueck uebersteht.
pub const LEISTE_AUDIO_H: u32 = 28;

/// Die Geometrie der Tonleiste ueber einer Zeile.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **neben** [`geometrie`] steht und
/// nicht darin als Parameter: [`geometrie`] ist von VIDEO_17 bis VIDEO_21
/// gemessen — 10 Tests, die Leiste liegt im Bild, der Griff rueckt, der
/// Griff liegt auf dem Balken. Eine Aenderung an ihr ist eine Aenderung an
/// etwas, das gruen ist. Die Tonleiste ist eine **zweite** Sache und
/// bekommt eine **zweite** Rechnung.
///
/// GEMESSENE ENTSCHEIDUNG, warum **kein** Vollbild-Knopf: `<audio>` hat
/// keinen Bildbereich, also gibt es nichts, was Vollbild werden koennte.
/// GEMESSENE ENTSCHEIDUNG, warum das **ohne** Aenderung an [`treffer`]
/// geht: der Vollbild-Knopf bekommt ein Rechteck der Groesse 0x0. Damit
/// ist `enthaelt` immer `false`, und `treffer` gibt `Vollbild` nie zurueck
/// — **ohne** dass `treffer` eine Zeile geaendert wurde. Ein Test, der
/// `treffer` prueft, gilt damit fuer **beide** Leisten.
pub fn geometrie_fuer_audio(breite: u32) -> Leiste {
    let h = LEISTE_AUDIO_H;
    let rand = 4;
    let kx = rand;
    let lx = kx + KNOPF + 8;
    // GEMESSENE ENTSCHEIDUNG, warum rechts **nur** der Lautstaerke-Knopf
    // sitzt und die Zeit davor: bei 28 Pixeln Hoehe ist kein Platz fuer
    // zwei 32-Pixel-Knoepfe nebeneinander — sie wuerden uebereinander
    // liegen. Also **ein** Knopf rechts, und die Zeit steht zwischen
    // Balken und Lautstaerke.
    let vx = breite.saturating_sub(rand + KNOPF);
    let balken_b = vx.saturating_sub(8 + 44).saturating_sub(lx);
    let zy = y_zentrum(h);
    Leiste {
        abspielen: Rechteck { x: kx, y: h / 2 + h / 2 - KNOPF / 2, b: KNOPF, h: KNOPF },
        balken: Rechteck { x: lx, y: zy, b: balken_b, h: 5 },
        griff: Rechteck { x: lx, y: zy.saturating_sub(6), b: 12, h: 12 },
        lautstaerke: Rechteck { x: vx - KNOPF, y: h / 2 + h / 2 - KNOPF / 2, b: KNOPF, h: KNOPF },
        // GEMESSENE ENTSCHEIDUNG, warum die Groesse **0x0** ist: siehe
        // Kopfkommentar. `treffer` prueft `enthaelt`, und ein Rechteck ohne
        // Flaeche enthaelt keinen Punkt.
        vollbild: Rechteck { x: 0, y: 0, b: 0, h: 0 },
        zeit: Rechteck { x: lx + balken_b + 8, y: zy - 1, b: 44, h: 11 },
    }
}

/// Die Mitte der Zeile fuer einen Balken von `h` Pixeln.
fn y_zentrum(h: u32) -> u32 {
    h / 2 + h / 2 - 6
}

/// Zeichnet die Tonleiste und gibt die benutzte Geometrie zurueck.
///
/// **Dieselbe Zusage wie bei [`zeichnen`]**: was zurueckgegeben wird, ist
/// genau das, wonach [`treffer`] sucht.
pub fn zeichne_fuer_audio(b: &mut Bild, z: &Zustand) -> Leiste {
    let g = geometrie_fuer_audio(b.breite);
    let weiss = [255u8, 255, 255, 255];

    // Der Hintergrund: **heller**, weil die Tonleiste nicht auf einem Bild
    // liegt. GEMESSENE ENTSCHEIDUNG, warum nicht schwarz: ein schwarzes
    // Band auf weissem Seitenhintergrund ist ein Loch in der Seite.
    fuellung(b, Rechteck { x: 0, y: 0, b: b.breite, h: g.abspielen.y + g.abspielen.h / 2 }, [0x28, 0x28, 0x2E, 0xFF]);

    // Play/Pause — dieselben Zeichen wie bei [`zeichnen`], damit der
    // Nutzer nicht zwei Symbole fuer dieselbe Sache lernt.
    match z.laeuft {
        false => {
            let zx = g.abspielen.x + 10;
            let zy0 = g.abspielen.y + 8;
            for i in 0..16u32 {
                fuellung(b, Rechteck { x: zx + i / 2, y: zy0 + i, b: 16 - i / 2, h: 1 }, weiss);
            }
        }
        true => {
            fuellung(b, Rechteck { x: g.abspielen.x + 10, y: g.abspielen.y + 9, b: 4, h: 14 }, weiss);
            fuellung(b, Rechteck { x: g.abspielen.x + 18, y: g.abspielen.y + 9, b: 4, h: 14 }, weiss);
        }
    }

    let bis = balken_bis(&g, z);
    fuellung(b, g.balken, [255, 255, 255, 0x40]);
    fuellung(b, Rechteck { x: g.balken.x, y: g.balken.y, b: bis, h: g.balken.h }, weiss);
    let griff_x = (g.balken.x + bis.saturating_sub(g.griff.b / 2))
        .max(g.balken.x)
        .min(g.balken.x + g.balken.b.saturating_sub(g.griff.b));
    let griff = Rechteck { x: griff_x, y: g.griff.y, b: g.griff.b, h: g.griff.h };
    fuellung(b, griff, [0, 0, 0, 0x80]);
    fuellung(b, Rechteck { x: griff.x + 1, y: griff.y + 1, b: griff.b - 2, h: griff.h - 2 }, weiss);

    // Der Lautstaerke-Knopf: dasselbe Zeichen wie beim Video.
    let sx = g.lautstaerke.x + 8;
    let sy = g.lautstaerke.y + 12;
    fuellung(b, Rechteck { x: sx, y: sy, b: 4, h: 8 }, weiss);
    if z.ton_an && z.lautstaerke > 0.0 {
        for i in 0..6u32 {
            let h = 2 + i;
            fuellung(b, Rechteck { x: sx + 5 + i, y: sy + 8 - h / 2, b: 1, h }, weiss);
        }
    } else {
        for i in 0..10u32 {
            fuellung(b, Rechteck { x: sx + 6 + i, y: sy + 3 + i, b: 2, h: 2 }, [255, 80, 80, 255]);
        }
    }

    let t = format!("{} / {}", zeit_text(z.zeit), zeit_text(z.dauer));
    ziffern(b, g.zeit.x, g.zeit.y, &t, weiss);

    // GEMESSENE ENTSCHEIDUNG: die zurueckgegebene Leiste traegt den
    // **bewegten** Griff — sonst waere der Klick auf ihn ein Sprung.
    Leiste { griff, ..g }
}
