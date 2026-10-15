//! Grid-Teilmenge als reine Layout-Funktion (C15, W6).
//!
//! grid-template-columns mit px/festen Laengen und fr, repeat(n, ...),
//! gap, Platzierung strikt in Reihenfolge (keine explizite Platzierung,
//! keine Zeilen-Definition — Zeilenhoehe = hoechstes Kind, gestreckt
//! wird auf Zellenhoehe). Reine Funktion: Kinderzahl gedeckelt
//! (MAX_RASTER_ZELLEN, M4), kein Panic (nicht-endlich -> 0).

use super::typen::SpaltenMass;
use super::{MAX_PX, MAX_RASTER_SPALTEN, MAX_RASTER_ZELLEN};

/// Saeubert eine Eingabelaenge (endlich, 0..MAX_PX).
fn sauber_pos(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.clamp(0.0, MAX_PX)
}

/// Ein Grid-Kind: nur die Inhaltshoehe (Breite kommt aus der Spalte).
/// Die Zellenhoehe ist das Zeilenmaximum (stretch); wer weniger braucht,
/// richtet sich oben aus (dokumentierte Teilmenge, kein align-self).
#[derive(Debug, Clone, Copy)]
pub struct RasterKind {
    /// Inhaltshoehe in px (>= 0 nach Kappung).
    pub hoch: f32,
    /// `align-self` des Kindes (§6.6, §10.4) — die BLOCKACHSE.
    ///
    /// L4 (03.10.2026 09:5x). GEMESSENE ENTSCHEIDUNG, warum die Ausrichtung
    /// am KIND steht und nicht am Container: §6.2/§6.3 — `align-items` ist
    /// der Vorgabewert des Containers, `align-self` der Wert DES KINDES.
    /// Beide kommen in `Berechnet` an; hier werden sie je Kind gelesen.
    pub selbst_vertikal: crate::typen::FlexAusrichtung,
    /// `justify-self` des Kindes (§6.2, §10.3) — die QUERACHSE.
    pub selbst_waagerecht: crate::typen::FlexAusrichtung,
    /// **`height` ist `auto`?** §6.6: `align-self: stretch` gilt **nur**,
    /// wenn die Groesse `auto` ist. Ohne diese Flagge wusste
    /// `richte_kind` nicht, ob es strecken darf, und tat es **immer** —
    /// GEMESSENER FEHLER (`CSS_22.txt`, Fall 5): `height:86px` mit
    /// `grid-row: span 2` kam auf **178** statt 86.
    pub hoch_auto: bool,
    /// **`width` ist `auto`?** dasselbe auf der Querachse (§6.6,
    /// `justify-self: stretch`). GEMESSENER FEHLER (`CSS_15.txt`, Punkt 3):
    /// eine definite `width:80px` im Raster kam auf **960** = die ganze
    /// `1fr`-Spalte.
    pub breit_auto: bool,
    /// Inhaltsbreite in px (>= 0 nach Kappung).
    ///
    /// L4 (03.10.2026 05:4x). Dieselbe Form wie `hoch`: die Zahl kommt aus
    /// der Messung (`Sizes`, die Border-Box des Kindes) und nicht aus der
    /// Kaskade.
    ///
    /// GEMESSENER GRUND, warum sie gebraucht wird: die `auto`-Spur (§7.2)
    /// ist `max-content`, und §11.8 teilt den Rest zwischen den
    /// `auto`-Spalten auf. Damit haengt die Zahl der SPUR an der Breite
    /// des Kindes, und die stand vorher nirgends. GEMESSEN, dass es ohne
    /// sie keine Seite gebraucht haette: der `1fr`-Ersatz aus dem Bericht
    /// von 18:2x traf im Default-Fall (`justify-content: normal`) dieselbe
    /// Zahl — §11.8 dehnt namlich auch die `auto`-Spuren. Sie wird also
    /// erst dort sichtbar, wo der `1fr`-Ersatz danebenlag: 180 px
    /// Container, zwei `auto`-Spuren mit 100 px Inhalt — §11.8 hat keinen
    /// positiven Rest, also bleiben **100 und 100**; der `1fr`-Ersatz gab
    /// 90 und 90.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Breite am KIND steht und nicht
    /// in `SpaltenMass`: `SpaltenMass` wird in der KASKADE gebaut, und
    /// die Kaskade sieht das Kind nicht (sie sieht hoechstens die Breite
    /// der Ansicht). Die Layoutphase dagegen hat `Sizes`.
    pub breit: f32,
    /// Release 08.10.2026: die min-content-Breite des Kindes, wenn bekannt.
    /// `None` = unbekannt — dann gilt `breit` auch als Untergrenze (der
    /// Inhalt ist unumbrechbar, wie vorher). Eine `auto`-Spur schrumpft
    /// nie unter diese Zahl (CSS Grid 1 §11.5: die Basis ist min-content).
    pub breit_min: Option<f32>,
    /// Release 09.10.2026: `width: <p>%` — bezieht sich auf den
    /// GITTERBEREICH (Grid 1 §6.1), nicht auf den Container. Dazu die
    /// Polster+Rand-Breite, die bei `content-box` hinzukommt.
    pub breit_prozent: Option<(f32, f32)>,
    /// Wie viele Zeilen das Kind belegt (CSS Grid 1 §8.3).
    ///
    /// L4 (02.10.2026 22:3x). `0` und `1` bedeuten dasselbe: eine Zeile.
    /// GEMESSENE ENTSCHEIDUNG, warum `0` nicht „keine Zeile" heisst:
    /// `gitter_linie` liefert `0` fuer `auto`, und `0` als Zeilenzahl
    /// waere ein Kind ohne Zeile — ein Loch im Bild. Also wird `0` wie
    /// `1` gelesen, und der Aufrufer muss nichts umrechnen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zahl hier steht und nicht als
    /// Zaehler im Aufrufer: die Platzierung entscheidet ueber die
    /// Zellen, die ein Kind belegt (§8.5), und das ist dieselbe Zahl,
    /// die die Hoehe bestimmt (§8.3). Ein `RasterKind` ohne sie kann die
    /// Frage „belegt dieses Kind zwei Zeilen?" nicht beantworten.
    pub spann_zeilen: usize,
    /// Wie viele Spalten das Kind belegt (CSS Grid 1 §8.3). `0`/`1` =
    /// eine Spalte.
    ///
    /// L4 (22:4x): dasselbe fuer die Querachse. Die beiden Felder sind
    /// getrennt und nicht ein `(z, s)`-Paar, weil die Regeln getrennt
    /// gelesen werden — §8.5 zaehlt erst die Spalten einer Zeile und
    /// dann die Zeilen einer Spalte.
    pub spann_spalten: usize,
    /// Feste Startlinie auf der Zeilenachse (CSS Grid 1 §8.5), 1-basiert.
    ///
    /// L4 (03.10.2026 01:3x). `0` = `auto`, positiv = die Linie,
    /// negativ = von hinten gezaehlt (§8.3: „A negative integer refers to
    /// a line counted backwards from the end of the explicit grid").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier der ROHWERT steht und nicht der
    /// aufgeloeste Index: eine negative Linie laesst sich erst aufloesen,
    /// wenn die Zeilenzahl bekannt ist — und die kennt erst die
    /// Platzierung. §8.5 setzt die Linie als CURSOR-Position, also
    /// braucht der Cursor die Zahl, nicht das Ergebnis.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `i32` und nicht `Option<usize>`:
    /// `0` ist bei `gitter_linie` bereits der Wert fuer `auto`, und ein
    /// zweiter Nullwert (`None` plus `Some(0)`) waer eine Form, die man
    /// beim Lesen sortieren muss.
    pub zeile_start: i32,
    /// Feste Endlinie auf der Zeilenachse (§8.5). `0` = `auto`.
    ///
    /// L4 (01:3x): die Endlinie gehoert hierher und nicht in
    /// `spann_zeilen`, weil `grid-row: 2/-1` erst beim Platzieren eine
    /// Spannweite ergibt — die Zeilenzahl ist dann bekannt, vorher nicht.
    pub zeile_ende: i32,
    /// Feste Startlinie auf der Spaltenachse (§8.5). `0` = `auto`.
    pub spalte_start: i32,
    /// Feste Endlinie auf der Spaltenachse (§8.5). `0` = `auto`.
    pub spalte_ende: i32,
    /// Der Gebietsname aus `grid-area: <name>` (CSS Grid 1 §8.4).
    ///
    /// L4 (03.10.2026 02:8x). GEMESSENE ENTSCHEIDUNG, warum hier der
    /// NAME steht und nicht schon das Rechteck: das Rechteck entsteht aus
    /// der Vorlage des CONTAINERS, und die kennt der Plan des Kindes
    /// nicht. Also traegt das Kind den Namen, und wer die Vorlage hat,
    /// sucht das Rechteck.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein fester Ring ist: der Typ ist
    /// `Copy`, und `String` waere eine Allokation je Kind.
    pub gebiet: crate::typen::Gebiet,
    /// Release 11.10.2026: SICHERE Ausrichtung je Achse (waagerecht,
    /// senkrecht) — ist der Kasten groesser als seine Zelle, steht er am
    /// Anfang. So rechnen `margin: auto` (negativer freier Platz zaehlt als
    /// 0, CSS Grid 1 §10.2) und `safe` (CSS Align 3 §4.4).
    pub sicher: (bool, bool),
}

impl Default for RasterKind {
    fn default() -> Self {
        // Gemessener Inhalt ist keine gesetzte CSS-Groesse. Ohne explizite
        // Angabe sind beide Achsen auto und duerfen bei stretch wachsen.
        Self {
            hoch: 0.0,
            breit: 0.0,
            breit_min: None,
            breit_prozent: None,
            hoch_auto: true,
            breit_auto: true,
            selbst_vertikal: Default::default(),
            selbst_waagerecht: Default::default(),
            spann_zeilen: 0,
            spann_spalten: 0,
            zeile_start: 0,
            zeile_ende: 0,
            spalte_start: 0,
            spalte_ende: 0,
            gebiet: Default::default(),
            sicher: (false, false),
        }
    }
}

/// Gelegte Raster-Zelle in px (plus Zeile/Spalte, 0-basiert).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterZelle {
    /// X-Position.
    pub x: f32,
    /// Y-Position.
    pub y: f32,
    /// Zellenbreite (>= 0).
    pub breite: f32,
    /// Zellenhoehe = Zeilenhoehe (>= 0).
    pub hoehe: f32,
    /// Zeile (0-basiert).
    pub zeile: usize,
    /// Spalte (0-basiert).
    pub spalte: usize,
}

/// Legt `kinder` in Reihenfolge in ein Raster: `spalten` Spalten ueber
/// `container_breite`, `luecke` zwischen Spalten UND Zeilen.
///
/// Das ist der Stand vor L4 und der **Weg, den die alten Tests
/// benutzen** — sie behaupten weiterhin genau das, was sie behauptet
/// haben. Der echte Weg mit getrennten Achsen ist [`lege_raster_bei`].
pub fn lege_raster(
    container_breite: f32,
    spalten: &[SpaltenMass],
    luecke: f32,
    kinder: &[RasterKind],
) -> Vec<RasterZelle> {
    lege_raster_bei(container_breite, spalten, luecke, luecke, kinder)
}

/// Legt `kinder` in Reihenfolge in ein Raster, mit ZWEI Lücken:
/// `luecke_spalte` waagerecht, `luecke_zeile` senkrecht.
///
/// Rueckgabe: eine Zelle je Kind (hoechstens MAX_RASTER_ZELLEN).
/// Leere Spaltenliste oder 0 Kinder -> leer. fr ohne Rest -> 0.
///
/// # L4 (02.10.2026 08:1x): ZWEI LÜCKEN STATT EINER
///
/// GEMESSENER BEFUND, warum das keine Geschmacksfrage ist
/// (`crates/narsil-page/examples/l4_raster.rs`, Release, drei Spalten zu
/// 100 px in einem 600-px-Kasten, sechs Kinder):
///
/// ```text
/// ohne Lücke          x  40 / 140 / 240    y  40 /  60
/// gap: 20px          x  40 / 160 / 280    y  40 /  80
/// row-gap: 20px      x  40 / 140 / 240    y  40 /  60   <- tat NICHTS
/// column-gap: 20px   x  40 / 160 / 280    y  40 /  80   <- tat BEIDES
/// ```
///
/// `row-gap` wirkte nicht, `column-gap` trennte auch die Zeilen. Das ist
/// exakt das Gegenteil von CSS Grid 1 §10.1, wo `column-gap` den
/// Spaltenabstand meint und `row-gap` den Zeilenabstand — waagerecht
/// bzw. senkrecht, immer, unabhaengig von irgendetwas.
///
/// GEMESSENE ENTSCHEIDUNG, warum Grid hier ANDERS ist als Flex: Ein
/// Flex-Container hat eine Hauptachse, die `flex-direction` waehlt,
/// also muss die Zuordnung der Richtung folgen. Ein Grid hat KEINE
/// Hauptachse — `column-gap` ist per Definition waagerecht. Genau
/// deshalb ist die Zuordnung hier eine Zeile und kein `match`.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Wrapper und keine geaenderte
/// Signatur: 25 Aufrufstellen in den Tests behaupten das alte
/// Verhalten. Ein Wrapper laesst sie unveraendert gruen bleiben —
/// sie sind damit Regressionsschutz fuer den Fall „beide Achsen
/// gleich", den es weiterhin geben muss (`gap`).
/// Die BREITE, mit der eine Spur in der Zaehlung und als Basis
/// rechnet.
///
/// L4 (02.10.2026 08:6x): `minmax(min, max)` rechnet mit `min` — es
/// IST die Untergrenze, und genau die zaehlt bei `auto-fill` (CSS Grid
/// 1 §7.2.3: wiederhole, so oft es ohne Ueberlauf geht; massgeblich ist
/// das Minimum der Spur).
fn spur_breite(spur: &SpaltenMass) -> f32 {
    match spur {
        SpaltenMass::Fest(v) => sauber_pos(*v),
        SpaltenMass::MinMax { min, .. } => sauber_pos(*min),
        // `fr` und `auto` haben kein Minimum — 0.
        _ => 0.0,
    }
}

/// Die OBERgrenze einer Spur, oder `None`, wenn sie keine hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum `fr` KEINE Obergrenze ist: `1fr` ist
/// unbegrenzt, es nimmt, was uebrig ist. Eine Obergrenze nur fuer
/// `Fest` (und fuer das `max` von `minmax`) haelt die Verteilung so,
/// wie sie vorher war.
fn spur_obergrenze(spur: &SpaltenMass) -> Option<f32> {
    match spur {
        SpaltenMass::Fest(v) => Some(sauber_pos(*v)),
        SpaltenMass::MinMax { max, .. } => match **max {
            SpaltenMass::Fest(v) => Some(sauber_pos(v)),
            _ => None,
        },
        _ => None,
    }
}

/// Die Hoehe EINER Zeile nach ihrer Spur (CSS Grid 1 §8.3, §11.7).
///
/// L4 (02.10.2026 19:4x). GEMESSENER BEFUND, was vorher passierte: es
/// gab keine Zeilenspur, also war die Zeilenhoehe immer das groesste
/// Kind der Zeile — `grid-template-rows: 200px 200px` ergab bei 20 px
/// Inhalt **20 px** (GEMESSEN in `tests/position.rs`,
/// `l4_grid_template_rows_legt_die_zeilenhoehe`: `left: (20.0, 20.0)`).
///
/// GEMESSENE ENTSCHEIDUNG, warum `inhalt` bei KEINER Spur eine Rolle
/// spielt und der Fall trotzdem `Fest` zuerst ist: §8.3 macht die Spur
/// zum MASS der Zeile. Eine Spur mit einer Laenge hat keinen Platz, in
/// den der Inhalt hineinwachsen koennte — der Inhalt laeuft ueber.
/// Genau das ist der Fall, an dem man die Regel sieht: bei `auto`
/// waere immer die Inhaltshoehe die Zeilenhoehe.
///
/// GEMESSENE ENTSCHEIDUNG, warum `fr` hier die Inhaltshoehe ergibt und
/// nicht einen Bruchteil davon: §11.7 gibt den freien Platz nur
/// verteilt, „if the grid container has a definite size". Ohne
/// festgelegte Containerhoehe gibt es keinen freien Platz, also bleibt
/// die Spur bei ihrer Basis — und die ist die Inhaltshoehe. Das ist
/// keine Vereinfachung, das ist die Regel fuer den Fall, dass es keine
/// Containerhoehe gibt.
///
/// GEMESSENE ENTSCHEIDUNG, warum `MinMax` den Inhalt als BASIS nimmt und
/// nicht als Zahl: §11.7 nennt fuer eine `minmax`-Spalte dieselbe
/// Zweiteilung — Basis aus dem Inhalt (bzw. dem Minimum, das groessere),
/// Obergrenze aus `max`. Die Obergrenze greift auch ohne freien Platz,
/// weil sie eine Kappung ist und keine Verteilung.
/// §6.6 / §10.3 / §10.4: die Lage des KINDES in seiner Rasterzelle.
///
/// L4 (03.10.2026 09:5x). `stretch` (der Anfangswert) ist genau das, was
/// die Platzierung ohnehin rechnet — deshalb ist der Vergleich mit dem
/// Zustand von heute die **Gegenprobe** dieses Punktes.
///
/// GEMESSENER BEFUND, dass die Regel fehlte: Release,
/// `examples/l4_raster.rs`, Block AA (Zelle 300x100, Kind 40x20):
/// `stretch`, `center`, `flex-start`, `flex-end` und
/// `justify-self: center` ergaben ALLE `b 300 h 100`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion ist und nicht
/// `inline` an zwei Stellen: die Zellen werden an zwei Stellen gebaut
/// (die alte Bahn und der Cursor), und zwei Kopien einer Ausrichtungs-
/// regel sind zwei Orte, an denen `center` drinsteht.
///
/// Gesetzte Groessen werden nicht auf die Rasterflaeche gekappt: Grid 1
/// §6.2 erlaubt Ueberlauf. Beschnitt ist eine separate overflow-Regel.
///
/// GEMESSENE EINSCHRAENKUNG: `baseline` verhaelt sich wie `stretch`. Das
/// ist der Zustand von heute; die Grundlinie ist nicht gebaut, und eine
/// erfundene Ausrichtung waere eine Erfindung im Bild.
fn richte_kind(k: &RasterKind, x: f32, y: f32, breite: f32, hoehe: f32) -> (f32, f32, f32, f32) {
    use crate::typen::FlexAusrichtung as A;
    //
    // L4 (04.10.2026 05:2x): §6.6 — `stretch` gilt **nur bei `auto`**.
    //
    // GEMESSENE FEHLER, die das behebt:
    //   * `CSS_22.txt` Fall 5: `height:86px` mit `grid-row: span 2` kam auf
    //     **178** = zwei Zeilen (86 + 6 Luecke + 86). Firefox: **86**.
    //   * `CSS_15.txt` Punkt 3: `width:80px` im Raster kam auf **960** = die
    //     ganze `1fr`-Spalte. Firefox: **80**.
    //
    // GEMESSENE URSACHE, und sie stand in diesem `match`: der Zweig
    // `A::Strecke | A::Grundlinie => breite` bzw. `=> hoehe` **gewann immer**,
    // egal ob eine Groesse gesetzt war. `RasterKind` hatte kein Feld, das
    // `auto` von einer gesetzten Groesse unterscheidet — es fehlte nicht die
    // Regel, sondern das **Wissen**.
    let b = match k.selbst_waagerecht {
        A::Strecke | A::Grundlinie if k.breit_auto => breite,
        _ => match k.breit_prozent {
            Some((p, zusatz)) if p.is_finite() => sauber_pos(breite * p / 100.0 + zusatz),
            _ => sauber_pos(k.breit),
        },
    };
    let h = match k.selbst_vertikal {
        A::Strecke | A::Grundlinie if k.hoch_auto => hoehe,
        _ => sauber_pos(k.hoch),
    };
    let xx = match k.selbst_waagerecht {
        _ if k.sicher.0 && b > breite => x,
        A::Mitte => x + (breite - b) / 2.0,
        A::Ende => x + (breite - b),
        _ => x,
    };
    let yy = match k.selbst_vertikal {
        _ if k.sicher.1 && h > hoehe => y,
        A::Mitte => y + (hoehe - h) / 2.0,
        A::Ende => y + (hoehe - h),
        _ => y,
    };
    // center/end koennen bei Ueberlauf vor dem Anfang der Zelle liegen.
    // Positionen sind vorzeichenbehaftet, Groessen weiterhin nichtnegativ.
    let koordinate = |v: f32| {
        if v.is_finite() { v.clamp(-MAX_PX, MAX_PX) } else { 0.0 }
    };
    (koordinate(xx), koordinate(yy), sauber_pos(b), sauber_pos(h))
}

fn zeilen_hoehe(spur: Option<&SpaltenMass>, inhalt: f32) -> f32 {
    let inhalt = sauber_pos(inhalt);
    let Some(spur) = spur else {
        // Implizite Zeile (§7.5): `grid-auto-rows` ist `auto`.
        return inhalt;
    };
    match spur {
        SpaltenMass::Fest(v) => sauber_pos(*v),
        SpaltenMass::MinMax { min, .. } => {
            let basis = inhalt.max(sauber_pos(*min));
            match spur_obergrenze(spur) {
                Some(grenze) => basis.min(grenze),
                None => basis,
            }
        }
        // `fr` und `auto` — siehe die Begruendung oben: ohne feste
        // Containerhoehe ist die Basis die Inhaltshoehe.
        SpaltenMass::Anteil(_) | SpaltenMass::Auto { .. } => inhalt,
        // L4 (03.10.2026 05:4x): die `auto`-SPUR (§7.2) gibt hier
        // dieselbe Zahl wie `fr` — und aus demselben Grund: es gibt keine
        // freie Stelle zum Verteilen, wenn der Container keine Hoehe
        // hat, also bleibt die Spur bei ihrer Basis, dem Inhalt.
        //
        // GEMESSENE EINSCHRAENKUNG, und sie ist echt: §11.8 dehnt auch
        // die ZEILEN mit `auto`-Obergrenze um den Rest — aber nur, wenn
        // der Container eine FESTE Hoehe hat. `lege_raster_bei_dicht`
        // rechnet die Zeilenhoehe immer aus dem Inhalt, und die Dehnung
        // der Zeilenachse ist nicht gebaut.
        SpaltenMass::Inhalt => inhalt,
        // L4 (09:3x): §10.1 gilt fuer die Querachse genauso, aber der
        // Rasterfluss kennt die CONTAINERHOEHE nicht — und ein Rest zum
        // Verteilen gibt es ohne sie auch nicht. Also der Inhalt, wie bei
        // `fr` und `auto`.
        //
        // GEMESSENE EINSCHRAENKUNG: bei fester Containerhoehe waere die
        // Zeile `0.5 * hoehe`. Das ist nicht gebaut und steht im Bericht.
        SpaltenMass::Prozent(_) => inhalt,
    }
}

/// Die Hoehe der Zeile `zeile` — aus der VORLAGE oder, wenn sie nicht
/// darin steht, aus `grid-auto-rows` (CSS Grid 1 §7.5).
///
/// L4 (02.10.2026 21:6x). GEMESSENER BEFUND, warum es eine eigene
/// Funktion ist und nicht ein `zeilen.get(zeile)` mit einer
/// Verlaengerung: es ist EINE Regel fuer ZWEI Quellen — die Vorlage
/// (`grid-template-rows`, §8.3) und die impliziten Spuren
/// (`grid-auto-rows`, §7.5) — und eine Regel gehoert an eine Stelle.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Auto-Liste ZYKLISCH gelesen wird:
/// §7.5 sagt ausdruecklich, dass sich das Muster "as necessary" wiederholt.
/// Ohne das gaebe es ab der dritten impliziten Zeilen keine Spur mehr,
/// und das waere eine stille Luecke.
///
/// GEMESSENE ENTSCHEIDUNG, warum nur `Fest` gelesen wird und `fr`/
/// `minmax`/`auto` nicht: eine implizite Zeile ist nach §7.5 `auto`, und
/// `auto` als Spur-Groesse ist der Inhalt. `fr` braucht eine feste
/// Containerhoehe (§11.7), die es nicht gibt; `minmax` als Spur-Groesse
/// waere erlaubt, aber nur die Obergrenze zu rechnen ist eine halbe
/// Sache.
fn auto_zeilen_hoehe(
    zeile: usize,
    vorlage: &[SpaltenMass],
    auto_vorlage: &[SpaltenMass],
    inhalt: f32,
) -> f32 {
    if zeile < vorlage.len() {
        return zeilen_hoehe(vorlage.get(zeile), inhalt);
    }
    if auto_vorlage.is_empty() {
        return zeilen_hoehe(None, inhalt);
    }
    let k = zeile - vorlage.len();
    match auto_vorlage[k % auto_vorlage.len()] {
        SpaltenMass::Fest(v) => sauber_pos(v),
        _ => zeilen_hoehe(None, inhalt),
    }
}

/// Zeilenrichtung — der Anfangswert `row` (CSS Grid 1 §8.5).
///
/// L4 (02.10.2026 11:3x): Hülle. Vorher war das der ganze Aufruf; jetzt
/// ist es der Aufruf mit `false`. Die Hülle bleibt, weil sieben
/// Aufrufstellen in `tests/position.rs` die alte Form behaupten — und
/// eine Abnahme, die nur die alte Form kennt, soll nicht an einer
/// Signatur zerbrechen.
pub fn lege_raster_bei(
    container_breite: f32,
    spalten: &[SpaltenMass],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinder: &[RasterKind],
) -> Vec<RasterZelle> {
    lege_raster_bei_zeilen(
        container_breite,
        spalten,
        &[],
        luecke_spalte,
        luecke_zeile,
        kinder,
        false,
    )
}

/// Wie [`lege_raster_bei`], aber mit der Richtung aus `grid-auto-flow`.
///
/// `spaltenfluss = true` heisst `grid-auto-flow: column`.
pub fn lege_raster_bei_fluss(
    container_breite: f32,
    spalten: &[SpaltenMass],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinder: &[RasterKind],
    spaltenfluss: bool,
) -> Vec<RasterZelle> {
    lege_raster_bei_zeilen(
        container_breite,
        spalten,
        &[],
        luecke_spalte,
        luecke_zeile,
        kinder,
        spaltenfluss,
    )
}

/// Wie [`lege_raster_bei_fluss`], aber mit den Zeilenspuren aus
/// `grid-template-rows` (CSS Grid 1 §8.3).
///
/// L4 (02.10.2026 19:4x). GEMESSENE ENTSCHEIDUNG, warum das eine neue
/// Funktion ist und kein weiterer Parameter: sieben Aufrufstellen in
/// `tests/position.rs` behaupten das alte Verhalten (Zeile = groesstes
/// Kind). Eine Aenderung an ihrer Signatur wuerde sie alle zu
/// Unrechtenden machen — sie pruefen etwas anderes. Also behaupten die
/// Hüllen `&[]` (keine Zeilenspuren, also `auto`, also die
/// Inhaltshoehe), und nur der Weg aus `LayoutStil` bringt die echten
/// Spuren mit. Das ist dasselbe Muster wie bei den Achsluecken
/// (L4 08:1x).
pub fn lege_raster_bei_zeilen(
    container_breite: f32,
    spalten: &[SpaltenMass],
    zeilen: &[SpaltenMass],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinder: &[RasterKind],
    spaltenfluss: bool,
) -> Vec<RasterZelle> {
    lege_raster_bei_autom(
        container_breite,
        spalten,
        zeilen,
        &[],
        &[],
        luecke_spalte,
        luecke_zeile,
        kinder,
        spaltenfluss,
    )
}

/// Wie [`lege_raster_bei_zeilen`], aber MIT den Groessen der impliziten
/// Spuren (CSS Grid 1 §7.3 `grid-auto-columns`, §7.5 `grid-auto-rows`).
///
/// L4 (02.10.2026 21:6x). GEMESSENER BEFUND, warum es eine neue Funktion
/// ist und keine zwei weiteren Parameter: die aeusseren Aufrufe
/// (`lege_raster_bei`, `lege_raster_bei_fluss`,
/// `lege_raster_bei_zeilen`) behaupten Verhalten ueber die impliziten
/// Spuren, und es sind inzwischen 16 Aufrufstellen. Eine Kette von
/// Hüllen ist billiger als 16 Aufrufe umzuschreiben.
pub fn lege_raster_bei_autom(
    container_breite: f32,
    spalten: &[SpaltenMass],
    zeilen: &[SpaltenMass],
    auto_spalten: &[SpaltenMass],
    auto_zeilen: &[SpaltenMass],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinder: &[RasterKind],
    spaltenfluss: bool,
) -> Vec<RasterZelle> {
    lege_raster_bei_gebiete(
        container_breite,
        spalten,
        zeilen,
        auto_spalten,
        auto_zeilen,
        &[],
        luecke_spalte,
        luecke_zeile,
        kinder,
        spaltenfluss,
    )
}

/// Wie [`lege_raster_bei_autom`], aber MIT den benannten Bereichen aus
/// `grid-template-areas` (§8.1).
///
/// L4 (03.10.2026 02:11x). GEMESSENE ENTSCHEIDUNG, warum es eine neue
/// Funktion ist und kein weiterer Parameter: die aeusseren Aufrufe
/// behaupten das alte Verhalten ohne Bereiche, und es sind inzwischen 20
/// Aufrufstellen in `tests/position.rs`.
pub fn lege_raster_bei_gebiete(
    container_breite: f32,
    spalten: &[SpaltenMass],
    zeilen: &[SpaltenMass],
    auto_spalten: &[SpaltenMass],
    auto_zeilen: &[SpaltenMass],
    gebiete: &[Vec<crate::typen::Gebiet>],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinder: &[RasterKind],
    spaltenfluss: bool,
) -> Vec<RasterZelle> {
    lege_raster_bei_dicht(
        container_breite,
        spalten,
        zeilen,
        auto_spalten,
        auto_zeilen,
        gebiete,
        luecke_spalte,
        luecke_zeile,
        kinder,
        spaltenfluss,
        false,
    )
}

/// Wie [`lege_raster_bei_gebiete`], aber mit `dense` (§8.5).
///
/// L4 (03.10.2026 04:2x). GEMESSENE ENTSCHEIDUNG, warum es eine neue
/// Funktion ist und kein weiterer Parameter: es sind inzwischen 24
/// Aufrufstellen in `tests/position.rs`, und die aeusseren behaupten das
/// alte Verhalten (`sparse`, der Anfangswert).
pub fn lege_raster_bei_dicht(
    container_breite: f32,
    spalten: &[SpaltenMass],
    zeilen: &[SpaltenMass],
    auto_spalten: &[SpaltenMass],
    auto_zeilen: &[SpaltenMass],
    gebiete: &[Vec<crate::typen::Gebiet>],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinder: &[RasterKind],
    spaltenfluss: bool,
    dicht: bool,
) -> Vec<RasterZelle> {
    lege_raster_spuren(
        container_breite,
        spalten,
        zeilen,
        auto_spalten,
        auto_zeilen,
        gebiete,
        luecke_spalte,
        luecke_zeile,
        kinder,
        spaltenfluss,
        dicht,
        None,
    )
    .0
}

/// Wie [`lege_raster_bei_dicht`], dazu die SPALTEN-SPUREN des Rasters als
/// `(x, Breite)` relativ zum Inhalt — leer im Spaltenfluss.
///
/// Release 11.10.2026: Masonry (`display: grid-lanes`, CSS Grid 3) braucht
/// die Spuren selbst, nicht nur die Zellen: eine Spur, in der kein
/// einspaltiges Element liegt (nur Spannen, oder leer), war aus den Zellen
/// nicht abzulesen. GEMESSEN vorher (WPT `column-auto-repeat-016`): von
/// sieben Spuren kamen fuenf an, die Spannen rutschten nach links.
pub fn lege_raster_spuren(
    container_breite: f32,
    spalten: &[SpaltenMass],
    zeilen: &[SpaltenMass],
    auto_spalten: &[SpaltenMass],
    auto_zeilen: &[SpaltenMass],
    gebiete: &[Vec<crate::typen::Gebiet>],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinder: &[RasterKind],
    spaltenfluss: bool,
    dicht: bool,
    // Release 11.10.2026: der Platz fuer die ZEILEN — `Some(hoehe)` bei
    // definiter Hoehe (`Some(0.0)` ohne: eine Wiederholung, kein `fr`-Rest),
    // `None` = das alte Verhalten (Zeilen ohne Wiederholung und `fr`).
    zeilen_platz: Option<f32>,
) -> (Vec<RasterZelle>, Vec<(f32, f32)>, Vec<(f32, f32)>) {
    let mut spuren: Vec<(f32, f32)> = Vec::new();
    let mut zeilen_spuren: Vec<(f32, f32)> = Vec::new();
    let zellen = lege_raster_kern(
        container_breite,
        spalten,
        zeilen,
        auto_spalten,
        auto_zeilen,
        gebiete,
        luecke_spalte,
        luecke_zeile,
        kinder,
        spaltenfluss,
        dicht,
        zeilen_platz,
        &mut spuren,
        &mut zeilen_spuren,
    );
    (zellen, spuren, zeilen_spuren)
}

#[allow(clippy::too_many_arguments)]
fn lege_raster_kern(
    container_breite: f32,
    spalten: &[SpaltenMass],
    zeilen: &[SpaltenMass],
    auto_spalten: &[SpaltenMass],
    auto_zeilen: &[SpaltenMass],
    gebiete: &[Vec<crate::typen::Gebiet>],
    luecke_spalte: f32,
    luecke_zeile: f32,
    kinder: &[RasterKind],
    spaltenfluss: bool,
    dicht: bool,
    zeilen_platz: Option<f32>,
    spuren: &mut Vec<(f32, f32)>,
    // Release 11.10.2026: die Zeilenspuren `(y, Hoehe)` — fuer den
    // Rasterbereich absolut positionierter Elemente (CSS Grid 1 §9.1).
    zeilen_spuren: &mut Vec<(f32, f32)>,
) -> Vec<RasterZelle> {
    let c = sauber_pos(container_breite);
    let gap = sauber_pos(luecke_spalte);
    let gap_zeile = sauber_pos(luecke_zeile);
    // Release 11.10.2026: `repeat(auto-fill | auto-fit, …)` auch fuer ZEILEN
    // (CSS Grid 1 §7.2.3) — gegen die definite Hoehe, sonst EINE Wiederholung.
    // GEMESSEN vorher: eine Wiederholungszeile war EINE Zeile so hoch wie ihr
    // Inhalt (WPT `row-auto-repeat-*`; die Masonry-Referenzen `grid-template-
    // rows: repeat(auto-fit, 50px)` zeigten ein einziges Element).
    let (zeilen_aus, zeilen_fit) = match zeilen_platz {
        Some(h) if zeilen.iter().any(|z| matches!(z, SpaltenMass::Auto { .. })) => {
            auszaehlen(sauber_pos(h), gap_zeile, zeilen)
        }
        _ => (zeilen.to_vec(), vec![false; zeilen.len()]),
    };
    let zeilen = zeilen_aus.as_slice();
    // L4 (08:4x): DIE WIEDERHOLUNEN ZAEHLEN. Muss VOR der Breiten-
    // rechnung stehen, weil dort `spalten.len()` die Spaltenzahl IST.
    // L4 (02:11x): `spalten0` ist die Vorlage PLUS die von den Bereichen
    // geforderten Spalten. GEMESSENE ENTSCHEIDUNG, warum die Ergaenzung
    // `Auto` ist: §8.1 macht die nicht genannten Spalten zu `auto`, und
    // `auto` als Spur heisst Inhalt — nicht 0.
    let mut spalten0: Vec<SpaltenMass> = spalten.to_vec();
    if !gebiete.is_empty() {
        let breit = gebiete.iter().map(Vec::len).max().unwrap_or(0);
        let n_spalten = spalten.len().max(breit).min(MAX_RASTER_SPALTEN);
        while spalten0.len() < n_spalten {
            // L4 (03.10.2026 05:5x): HIER STAND EIN `1fr`-ERSATZ. Die
            // Zeilen darunter bleiben als Befund stehen, weil sie eine
            // Zahl behaupten, die falsch war.
            //
            // GEMESSENER BEFUND, dass die Form nicht ankam: §8.1 macht
            // die nicht genannten Spalten zu `auto` (§7.3), und `RasterKind`
            // trug nur die HOEHE — also war die Inhaltsbreite an dieser
            // Stelle nicht messbar, und der Ersatz war `1fr`.
            //
            // GEMESSENE ENTSCHEIDUNG, warum `1fr` damals nicht falsch war
            // und trotzdem ersetzt werden musste (Korrektur an mir
            // selbst, 05:3x): §11.8 dehnt die Spalten mit `auto`-Obergrenze
            // um den Rest GLEICHMAESSIG, und `justify-content` hat den
            // Anfangswert `normal`. Der `1fr`-Ersatz traf also im
            // Default-Fall dieselbe Zahl — GEMESSEN in Block R2: alle
            // Spalten 300 px, was §11.8 richtig findet. Der Bericht von
            // 18:2x nannte das einen Fehler („300 statt 100"); das war die
            // FALSCHE DEUTUNG einer RICHTIGEN Messung. Siehe
            // `typen.rs`, `SpaltenMass::Inhalt`.
            //
            // GEMESSEN, WO der Ersatz danebenlag (also wo er doch falsch
            // war): `tests/position.rs`
            // `l4_bereichsspalten_sind_auto_spuren_kein_fr_ersatz` — 180 px
            // Container, zwei Bereiche mit 100 px Inhalt. §11.8 hat keinen
            // positiven Rest, also **100 und 100**; der `1fr`-Ersatz gab
            // **90 und 90** (GEMESSEN rot, vor dieser Aenderung). Und bei
            // verschiedenen Inhalten (100 und 50 in 600 px) sind es §11.8
            // 325 und 275 gegen `1fr` 300 und 300.
            spalten0.push(SpaltenMass::Inhalt);
        }
    }
    // Release 08.10.2026: ohne Spaltenvorlage gibt es KEINE expliziten
    // Spalten — der Spaltenfluss legt dann nur implizite an (siehe unten).
    let vorlage_leer = spalten0.is_empty();
    let gebiets_rechtecke = gebiets_rechtecke(gebiete);
    let (spalten, fit) = auszaehlen(c, gap, &spalten0);
    // L4 (03.10.2026 02:11x): DIE SPALTENZAHL AUS DEN BEREICHEN (§8.1).
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Bereiche die Spaltenzahl VORGEBEN
    // duerfen: §8.1 sagt, `grid-template-areas` definiert Zeilen UND
    // Spalten — die breiteste Zeile der Liste ist die Spaltenzahl. Ohne
    // das waere `grid-template-areas: "a b"` ohne `grid-template-columns`
    // ein einspaltiges Raster, und alle Gebiete ausser dem ersten laegen
    // ausserhalb.
    // L4 (02.10.2026 19:5x): OHNE SPALTEN IST EINE IMPLIZITE VOLLE SPALTE
    // (CSS Grid 1 §7.2.1: "the implicit grid... contains one column if no
    // explicit column track is specified").
    //
    // GEMESSENER BEFUND, was hier vorher stand: `if ncol == 0 { return }`
    // — also KEINE Zelle. GEMESSEN am 02.10. 19:5x in
    // `tests/position.rs`, `l4_minmax_auf_der_zeilenachse`:
    // `grid-template-rows:minmax(200px,400px)` ohne `grid-template-
    // columns` lieferte `index out of bounds: the len is 0 but the index
    // is 0`.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das hier und nicht in der Engine:
    // §7.2.1 ist eine Raster-Regel, und `lege_raster_bei_zeilen` ist die
    // Funktion, die §8.3 anwendet. Die Engine hatte dieselbe Ersetzung
    // (19:4x) als eigene Zeile — zwei Orte, eine Regel. Hier ist der
    // eine Ort.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Breite `c` (die Containerbreite)
    // ist: §7.2.1 sagt fuer die implizite Spur „the size of the grid
    // container". Das ist die Breite, die hier schon bereinigt wurde.
    //
    // GEMESSENER EIGENER FEHLER (19:6x), weil er eine Warnung verdient:
    // beim Zuruecksetzen einer A/B-Messung habe ich einen Zwischenstand
    // von 19:4x ueber diese Aenderung gelegt — der A/B-Vergleich war
    // fuer die Zeilenhoehe, und die Datei war der Anker. GEMESSEN hat es
    // `c15_raster_ohne_kinder_oder_spalten_ist_leer`: `index out of
    // bounds: the len is 0 but the index is 0`. **Ein A/B-Zwischenstand
    // ist kein Weg zum Zuruecksetzen; es ist ein Weg zum Verlieren.**
    let (spalten, fit) = if spalten.is_empty() {
        (vec![SpaltenMass::Fest(c)], vec![false; 1])
    } else {
        (spalten, fit)
    };
    let spalten = spalten.as_slice();
    // L4 (02.10.2026 10:5x): DIE KOLLABIEREN SPUREN (§11.7.1).
    //
    // GEMESSENER BEFUND, was ohne das passiert
    // (`examples/l4_raster.rs`, Release, 600-px-Kasten, zwei Kinder):
    //
    //     repeat(auto-fill, minmax(200px, 1fr))   3 Spalten a 200 px
    //     repeat(auto-fit,  minmax(200px, 1fr))   3 Spalten a 200 px
    //
    // §11.7.1: "collapse any empty auto-fit tracks" — danach soll die
    // dritte Spur 0 sein und der Platz an die beiden anderen gehen.
    //
    // GEMESSENER FEHLER MEINER ERSTEN REGEL, gefunden an der Messung
    // (es kamen 1 statt 2 Kinder heraus): ich hatte die belegten Spuren
    // mit `ceil(n / ncol)` berechnet. Bei 2 Kindern und 3 Spalten gibt
    // das 1 — und damit wurde Spur 1 kollabiert, auf der aber das
    // ZWEITE Kind liegt. Die Plazierung ist `i % ncol`, also sind
    // `min(n, ncol)` Spalten belegt, nicht `ceil(n/ncol)`.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die KOLLAPSE VOR der
    // Breitenrechnung passieren und nicht danach: nachher waeren die
    // Breiten schon verteilt und der frei gewordene Platz muesste von
    // Hand umverteilt werden. Vorher ist es eine Spur mit Breite 0 —
    // und die bestehende Rechnung gibt den Platz automatisch an die
    // `fr`-Spuren, weil `fest` dann kleiner ist.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die ZAEHLUNG unveraendert bleibt:
    // §11.7.1 kollabiert nach dem Zaehlen. `auto-fit` erzeugt also
    // dieselbe Zahl wie `auto-fill` und unterscheidet sich erst danach
    // — genau das sagt `auto_fit_zaehlt_wie_auto_fill`.
    let ncol = spalten.len().min(MAX_RASTER_SPALTEN);
    let n = kinder.len().min(MAX_RASTER_ZELLEN);
    let belegt_spuren = n.min(ncol);
    let kollabiert: [bool; 32] = std::array::from_fn(|i| {
        i < ncol && i >= belegt_spuren && fit.get(i).copied().unwrap_or(false)
    });
    let mut zellen: Vec<RasterZelle> = Vec::new();
    if ncol == 0 || n == 0 {
        return zellen;
    }
    zellen.reserve(n);
    // Spaltenbreiten: fest zuerst, Rest an fr (Anteil/Total).
    //
    // L4 (03.10.2026 05:4x): VOR der Schleife die INHALTSBREITE JE SPUR.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Breite gebraucht wird: die
    // `auto`-Spur (§7.2) ist `max-content`, und §11.8 („Stretch auto
    // Tracks") dehnt sie um den REST — geteilt wird also
    // (Container − Inhalte) durch n. Die Zahl der Spur haengt also an
    // ihrem Inhalt, und der Inhalt steht am KIND (`RasterKind::breit`,
    // aus `Sizes` — dieselbe Form wie `hoch`).
    //
    // GEMESSENE ENTSCHEIDUNG, warum das hier und nicht in der Kaskade:
    // die Kaskade sieht den Container nicht. Hier stehen alle drei
    // Angaben: die Spalten (`ncol`), die Gebiete (`gebiets_rechtecke`) und
    // die Kinder mit ihrer Breite.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ein Kind OHNE Gebiet in die Spalte
    // `i % ncol` geht (§8.5, Zeilenfluss): der Cursor fuellt Zeile fuer
    // Zeile, und damit haengt die SPALTE nur vom Index ab. Im
    // Spaltenfluss ist es umgekehrt: `i / Zeilenzahl`.
    //
    // GEMESSENE EINSCHRAENKUNG, und sie ist echt: ein Kind mit SPANNE
    // ueber mehrere Spuren geht hier nur in die Spur, in der es BEGINNT.
    // Der Anteil an den weiteren Spuren fehlt. GEMESSEN ist das bisher
    // nicht, weil der Korpus keine `auto`-Spur mit einem Kind darin hat.
    let mut spur_inhalt: [f32; 32] = std::array::from_fn(|si| {
        if si >= ncol {
            return 0.0;
        }
        let mut breite: f32 = 0.0;
        for (ki, k) in kinder.iter().take(n).enumerate() {
            let im = gebiets_rechtecke
                .iter()
                .find(|t| t.name == k.gebiet && !k.gebiet.ist_leer());
            let passt = match im {
                Some(t) => t.breite <= 1 && si == t.spalte,
                None if spaltenfluss => {
                    let je_spalte = zeilen.len().max(1);
                    ki / je_spalte == si
                }
                None => ki % ncol.max(1) == si,
            };
            if passt {
                breite = breite.max(sauber_pos(k.breit));
            }
        }
        breite
    });
    // Release 08.10.2026: CSS Grid 1 §12.5 — ein Kind, das MEHRERE Spuren
    // ueberspannt, zaehlt oben nicht mit (Schritt 3 nimmt nur Kinder mit
    // Spannweite 1). Schritt 4: sein UEBERSCHUSS wird auf die ueberspannten
    // Inhaltsspuren verteilt — aber nie, wenn es eine `fr`-Spur ueberspannt.
    // GEMESSEN vorher: MDN-Karten `"source date" "title title" "summary
    // summary"` mit `1fr max-content` — die Zusammenfassung blies die
    // `max-content`-Spalte auf ihre ganze Textlaenge, `1fr` wurde 0, und
    // Quelle und Datum lagen uebereinander.
    for k in kinder.iter().take(n) {
        let Some(t) = gebiets_rechtecke.iter().find(|t| t.name == k.gebiet && !k.gebiet.ist_leer()) else {
            continue;
        };
        let ende = (t.spalte + t.breite).min(ncol);
        if t.breite <= 1 || t.spalte >= ende || spalten[t.spalte..ende].iter().any(|s| matches!(s, SpaltenMass::Anteil(_))) {
            continue;
        }
        let inhalt_spuren: Vec<usize> =
            (t.spalte..ende).filter(|si| matches!(spalten[*si], SpaltenMass::Inhalt)).collect();
        if inhalt_spuren.is_empty() {
            continue;
        }
        let schon: f32 = (t.spalte..ende)
            .map(|si| match spalten[si] {
                SpaltenMass::Fest(v) => sauber_pos(v),
                SpaltenMass::Prozent(f) => sauber_pos(f) * c,
                _ => spur_inhalt[si],
            })
            .sum::<f32>()
            + gap * (ende - t.spalte - 1) as f32;
        let rest = sauber_pos(k.breit) - schon;
        if rest > 0.0 {
            let je = rest / inhalt_spuren.len() as f32;
            for si in inhalt_spuren {
                spur_inhalt[si] += je;
            }
        }
    }
    // Release 08.10.2026: dieselbe Zuordnung fuer die MINDESTbreite.
    let spur_inhalt_min: [f32; 32] = std::array::from_fn(|si| {
        if si >= ncol {
            return 0.0;
        }
        let mut breite: f32 = 0.0;
        for (ki, k) in kinder.iter().take(n).enumerate() {
            let im = gebiets_rechtecke
                .iter()
                .find(|t| t.name == k.gebiet && !k.gebiet.ist_leer());
            let passt = match im {
                Some(t) => t.breite <= 1 && si == t.spalte,
                None if spaltenfluss => {
                    let je_spalte = zeilen.len().max(1);
                    ki / je_spalte == si
                }
                None => ki % ncol.max(1) == si,
            };
            if passt {
                breite = breite.max(sauber_pos(k.breit_min.unwrap_or(k.breit).min(k.breit)));
            }
        }
        breite
    });
    let mut fest = 0.0f32;
    let mut fr_gesamt = 0.0f32;
    for (si, s) in spalten.iter().take(ncol).enumerate() {
        if kollabiert[si] {
            continue;
        }
        match s {
            SpaltenMass::Fest(v) => fest += sauber_pos(*v),
            SpaltenMass::Anteil(f) => {
                if f.is_finite() && *f > 0.0 {
                    fr_gesamt += f.min(1_000_000.0);
                }
            }
            // L4 (08:4x): hier kann keine `Auto`-Spalte stehen. Der
            // Aufruf von `auszaehlen` am Anfang hat sie alle ersetzt,
            // weil es keine Breite fuer sie gibt und keine Zeile fuer
            // ihre Wiederholungen. Der Arm ist trotzdem noetig, weil
            // der Compiler das nicht weiss — und er steht hier, damit
            // jemand, der `auszaehlen` entfernt, an dieser Stelle
            // sichtbar scheitert und nicht an einer stillen Breite 0.
            SpaltenMass::Auto { .. } => {}
            // L4 (03.10.2026 05:4x): die `auto`-SPUR (§7.2) zahlt mit
            // ihrem INHALT in die feste Basis — §11.8 teilt danach nur
            // den Rest, und wer den Inhalt nicht abzieht, gibt ihn zweimal
            // aus: einmal als Basis und einmal im Rest.
            SpaltenMass::Inhalt => fest += spur_inhalt[si],
            // L4 (09:3x): §10.1 — der Prozentsatz rechnet gegen `c`, die
            // CONTAINERbreite. Die steht hier als Parameter bereit, und
            // genau deshalb wandert die Zahl als Faktor durch die Kaskade.
            SpaltenMass::Prozent(f) => fest += sauber_pos(*f) * c,
            // L4 (08:6x): `minmax` zahlt mit seiner UNTERgrenze in die
            // feste Basis. GEMESSENE ENTSCHEIDUNG, warum nicht die
            // Obergrenze: `minmax(270px, 1fr)` muss Platz fuer 270 px
            // machen, BEVOR der Rest verteilt wird — sonst kaeme der
            // Anteil aus einem Kasten, der schon zu klein ist.
            // L4 (08:6x): `minmax` zaehlt mit seiner UNTERgrenze in die feste
            // Basis UND mit einem `fr`-Maximum in die Gesamtfr.
            //
            // GEMESSENER FEHLER MEINER ERSTEN FASSUNG, gefunden vom Test
            // `l4_minmax_ist_im_typ_und_wird_gerechnet`: es stand nur
            // `fest += min`. Damit blieb `fr_gesamt` bei 0 und der Rest
            // wurde nie verteilt — GEMESSEN 270 px statt 300 px in einem
            // 600-px-Kasten. Also fehlte nicht eine Zeile, sondern eine
            // halbe Rechnung: eine `minmax`-Spur ist zugleich fest UND
            // anteilig.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die UNTERgrenze fest zaehlt
            // und nicht die Obergrenze: `minmax(270px, 1fr)` muss zuerst
            // Platz fuer 270 px machen, BEVOR der Rest verteilt wird —
            // sonst kaeme der Anteil aus einem Kasten, der schon zu
            // klein ist.
            SpaltenMass::MinMax { min, max } => {
                fest += sauber_pos(*min);
                if let SpaltenMass::Anteil(f) = **max
                    && f.is_finite()
                    && f > 0.0
                {
                    fr_gesamt += f.min(1_000_000.0);
                }
            }
        }
    }
    // L4 (10:5x): die Abstaende zwischen ZWEI KOLLABIERTEN Spalten
    // entfallen — eine Spur mit Breite 0 hat auch keinen Abstand zu
    // ihrer Nachbarin, sonst stuende da ein Luecken-Balken im Nichts.
    // GEMESSENE ENTSCHEIDUNG: hier wird die Zahl der SICHTBAREN
    // Spalten gezaehlt, nicht `ncol`.
    let sichtbar = (0..ncol).filter(|&i| !kollabiert[i]).count();
    let luecken = gap * (sichtbar as f32 - 1.0).max(0.0);
    // Release 08.10.2026: passen die `auto`-Spuren mit ihrem max-content
    // nicht in den Container, teilen sie sich den Platz im Verhaeltnis
    // ihres Inhalts (CSS Grid 1 §11.5: der Inhalt ist die Wachstumsgrenze,
    // nicht die Basis). GEMESSEN vorher: eine `auto`-Spalte mit einem
    // langen Absatz schob jede Nachbarspalte aus dem Bild.
    let inhalt_summe: f32 = spalten
        .iter()
        .take(ncol)
        .enumerate()
        .filter(|(i, s)| !kollabiert[*i] && matches!(s, SpaltenMass::Inhalt))
        .map(|(i, _)| spur_inhalt[i])
        .sum();
    let ohne_inhalt = fest - inhalt_summe;
    let frei_fuer_inhalt = (c - ohne_inhalt - luecken).max(0.0);
    if inhalt_summe > frei_fuer_inhalt && inhalt_summe > 0.0 {
        let min_summe: f32 = spalten
            .iter()
            .take(ncol)
            .enumerate()
            .filter(|(i, s)| !kollabiert[*i] && matches!(s, SpaltenMass::Inhalt))
            .map(|(i, _)| spur_inhalt_min[i].min(spur_inhalt[i]))
            .sum();
        // Zwischen Untergrenze (min-content) und Inhalt (max-content)
        // im selben Verhaeltnis; reicht nicht einmal die Untergrenze, bleibt
        // sie stehen (der Inhalt laeuft ueber, wie in Firefox).
        let anteil = if inhalt_summe - min_summe > 0.01 {
            ((frei_fuer_inhalt - min_summe) / (inhalt_summe - min_summe)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let mut neu_summe = 0.0f32;
        for (i, s) in spalten.iter().take(ncol).enumerate() {
            if matches!(s, SpaltenMass::Inhalt) && !kollabiert[i] {
                let unten = spur_inhalt_min[i].min(spur_inhalt[i]);
                spur_inhalt[i] = unten + (spur_inhalt[i] - unten) * anteil;
                neu_summe += spur_inhalt[i];
            }
        }
        fest = ohne_inhalt + neu_summe;
    }
    // Release 08.10.2026: §11.5 („Maximize Tracks") VOR §11.7 („Expand
    // Flexible Tracks"): eine `minmax(a, b)`-Spur mit fester Obergrenze
    // waechst zuerst bis `b`, erst der Rest geht an die `fr`-Spuren.
    // GEMESSEN vorher: die Inhaltsspalte von developer.mozilla.org
    // (`minmax(0, 48rem)` zwischen zwei `minmax(15rem, 1fr)`) blieb bei
    // ihrer Basis 0 — die `fr`-Spuren hatten den ganzen Rest schon
    // genommen, und der Artikel stand Wort fuer Wort untereinander.
    let mut zuwachs: [f32; 32] = [0.0; 32];
    {
        let mut frei = (c - fest - luecken).max(0.0);
        for _ in 0..ncol {
            let offen: Vec<usize> = (0..ncol)
                .filter(|&i| {
                    !kollabiert[i]
                        && matches!(spalten[i], SpaltenMass::MinMax { .. })
                        && spur_obergrenze(&spalten[i]).is_some_and(|g| {
                            let basis = match &spalten[i] {
                                SpaltenMass::MinMax { min, .. } => sauber_pos(*min),
                                _ => 0.0,
                            };
                            basis + zuwachs[i] < g - 0.01
                        })
                })
                .collect();
            if offen.is_empty() || frei <= 0.01 {
                break;
            }
            let je = frei / offen.len() as f32;
            for &i in &offen {
                let basis = match &spalten[i] {
                    SpaltenMass::MinMax { min, .. } => sauber_pos(*min),
                    _ => 0.0,
                };
                let grenze = spur_obergrenze(&spalten[i]).unwrap_or(basis);
                let dazu = je.min((grenze - basis - zuwachs[i]).max(0.0));
                zuwachs[i] += dazu;
                frei -= dazu;
            }
        }
        fest += zuwachs.iter().sum::<f32>();
    }
    let rest = (c - fest - luecken).max(0.0);
    let mut breiten: [f32; 32] = [0.0; 32];
    for (i, s) in spalten.iter().take(ncol).enumerate() {
        if kollabiert[i] {
            breiten[i] = 0.0;
            continue;
        }
        breiten[i] = match s {
            SpaltenMass::Fest(v) => sauber_pos(*v),
            SpaltenMass::Anteil(f) => {
                if fr_gesamt > 0.0 && f.is_finite() && *f > 0.0 {
                    rest * f.min(1_000_000.0) / fr_gesamt
                } else {
                    0.0
                }
            }
            // L4 (08:4x): siehe der gleiche Arm in der Schleife oben —
            // `auszaehlen` laesst hier keine `Auto`-Spalte durch.
            SpaltenMass::Auto { .. } => 0.0,
            // L4 (03.10.2026 05:4x): die `auto`-SPUR (§7.2) startet bei
            // ihrem INHALT. Der Rest kommt in §11.8 dazu — das ist ein
            // eigener Schritt und nicht `rest / n` hier, weil §11.5
            // (§`maximiere`) und §11.7 (`fr`) VOR §11.8 rechnen und der
            // Rest sich mit ihnen aendert.
            SpaltenMass::Inhalt => spur_inhalt[i],
            // L4 (09:3x): dieselbe Rechnung wie in der Schleife oben —
            // §10.1, und `c` ist der Container.
            SpaltenMass::Prozent(f) => sauber_pos(*f) * c,
            // L4 (08:6x): die Untergrenze PLUS der Anteil aus `max`,
            // wenn `max` ein `fr` ist, und auf `max` gekappt, wenn
            // `max` eine Laenge ist.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Obergrenze NACH der
            // Verteilung greift und nicht davor: `minmax(100px, 200px)`
            // in einem 600-px-Kasten mit zwei solchen Spuren hat 400 px
            // Platz und 200 gebraucht. Ohne Kappung waere jede Spur
            // 200 px (die Basis) — richtig. Mit zwei `minmax(100px,1fr)`
            // waere die Basis 200 und der Rest 400, jeder Anteil 200,
            // also 300 je Spur. Die Kappung greift nur, wenn `max` eine
            // Zahl ist; bei `fr` gibt es keine.
            SpaltenMass::MinMax { min, max } => {
                let basis = sauber_pos(*min) + zuwachs[i];
                let anteil = match **max {
                    SpaltenMass::Anteil(f) if fr_gesamt > 0.0 && f.is_finite() && f > 0.0 => {
                        rest * f.min(1_000_000.0) / fr_gesamt
                    }
                    _ => 0.0,
                };
                let roh = basis + anteil;
                match spur_obergrenze(s) {
                    Some(grenze) => roh.min(grenze).max(basis),
                    None => roh,
                }
            }
        };
    }
    // L4 (02.10.2026 10:0x): §11.5 MAXIMISE TRACKS.
    //
    // GEMESSENER BEFUND, was hier vorher fehlte: `minmax(100px, 200px)`
    // in einem 600-px-Kasten ergab **100 px** statt 200. Die Spur blieb
    // bei ihrer BASIS, weil es ohne `fr` nichts zu verteilen gab — aber
    // §11.5 says: distribute free space equally among all tracks,
    // FREEZING the fr ones. Eine Spur mit einer OBERGRENZE waere also
    // genau die, die wachsen darf.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `fr`-Spalten hier nicht mitkommen:
    // §11.5 nennt sie „frozen" — sie haben ihren Anteil aus der
    // Verteilung von §11.7 schon. Sie haben auch keine Obergrenze
    // (`spur_obergrenze` gibt `None`), also sind sie doppelt
    // ausgeschlossen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum eine SCHLEIFE und kein einzelner
    // Durchgang: zwei Spalten mit unterschiedlichem Zuwachs
    // (`minmax(100px,200px)` und `minmax(100px,500px)`) brauchen zwei
    // Runden — die kleine erreicht ihre Grenze und gibt danach nichts
    // mehr ab. Eine Schleife mit Abbruch bei „nichts hat sich mehr
    // geaendert" ist eine Zeile mehr und richtig.
    maximiere(&mut breiten, spalten, c, gap);
    // L4 (03.10.2026 05:4x): §11.8 „STRETCH AUTO TRACKS" — als LETZTER
    // Schritt, weil die Spec die Reihenfolge §11.5 → §11.7 → §11.8 nennt
    // und der Rest sich mit §11.5 geaendert hat.
    //
    // GEMESSENER BEFUND, warum das nicht in `maximiere` gehoert: dort
    // kommen nur Spalten MIT einer Obergrenze hinein, und `auto` hat per
    // Definition keine. Ein Zusatz `else` dort waere eine zweite Frage im
    // selben Arm — und §11.8 ist eine eigene Frage.
    dehne_auto_spuren(&mut breiten, spalten, c, gap);
    // Spalten-X-Positionen.
    let mut xpos: [f32; 32] = [0.0; 32];
    let mut x = 0.0f32;
    for i in 0..ncol {
        xpos[i] = x;
        x += breiten[i] + gap;
    }
    if !spaltenfluss {
        spuren.extend((0..ncol).map(|i| (xpos[i], breiten[i])));
    }
    // Zeilenweise platzieren (§8.3, §8.5).
    //
    // L4 (02.10.2026 22:4x): `idx` und `y` sind hier weg. Sie waren die
    // Zaehler des alten Blocks („welches Kind ist dran" und „wo ist die
    // Zeile"). Der Cursor traegt seine Lage in sich (`lage`), und die
    // y-Position steht in `ypos`. Zwei Zaehler, die nichts mehr zaehlen,
    // sind zwei Buchungen, die auseinanderlaufen koennen — GEMESSEN hat es
    // der Compiler: „unused variable" fuer beide.
    // L4 (02.10.2026 11:3x): DIE SPALTENRICHTUNG (§8.5).
    //
    // GEMESSENER BEFUND, was `column` ohne das hier tut (Release,
    // `l4_raster.rs`, 2 Spalten, 6 Kinder, vorher):
    //
    //     grid-auto-flow: row     3 Zeilen, 2 Spalten belegt
    //     grid-auto-flow: column  3 Zeilen, 2 Spalten belegt   <- gleich
    //
    // GEMESSENE ENTSCHEIDUNG, warum ALLE Kinder in Spalte 0 landen und
    // nicht auf 2 Spalten verteilt werden: §8.5 fuellt in der
    // Spaltenrichtung von oben nach unten und macht dann die naechste
    // SPALTE — aber nur, wenn das Gitter eine Zeilenzahl hat. Ohne
    // `grid-template-rows` gibt es keine, also erzeugt jedes Kind seine
    // EIGENE Zeile, und alle Kinder liegen in Spalte 0.
    //
    // Das ist die ehrliche Antwort und nicht die bequeme. Sie sieht
    // falsch aus (wer `column` benutzt, will doch Spalten), aber sie
    // ist die, die CSS gibt, und sie wird sofort falsch, sobald
    // `grid-template-rows` dazukommt — der naechste L4-Punkt.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Zeilenhoehe hier `kinder[i].hoch`
    // ist und nicht das Maximum einer Zeile: jedes Kind HAT seine eigene
    // Zeile, es gibt nichts zu vergleichen.
    // L4 (02.10.2026 21:2x): DIE ZEILENZAHL (§8.5 `grid-auto-flow:
    // column`).
    //
    // GEMESSENER BEFUND, was hier vorher stand: jedes Kind bekam seine
    // EIGENE Zeile, alle in `xpos[0]`. GEMESSEN (`tests/position.rs`,
    // `l4_spaltenfluss_braucht_die_zeilenvorlage`): 6 Kinder in 2 Spalten
    // und 2 Zeilen ergaben `[(0,0), (0,1), (0,2), (0,3), (0,4), (0,5)]`
    // statt `[(0,0), (0,1), (1,0), (1,1), (2,0), (2,1)]`. Der Grund stand
    // seit 11:3x im Kommentar: ohne Zeilenvorlage gab es keine
    // Zeilenzahl, und §8.5 fuellt die Spalte bis zur Zeilenzahl.
    //
    // GEMESSENE ENTSCHEIDUNG, warum eine LEERE Vorlage UNBEGRENZT viele
    // Zeilen ergibt und nicht eine: §7.3 erzeugt implizite Spuren, "as
    // many as necessary" — die Zeilenzahl ist also nicht begrenzt, und
    // §8.5 kann die Spalte nie voll machen. Deshalb bleibt alles in
    // Spalte 0, ein Kind je Zeile. Das ist die alte Behauptung von 11:3x,
    // und sie ist richtig — sie war nur damals falsch BEGRUENDET (die
    // Begründung war „keine Vorlage, also keine Zahl"; die Regel ist
    // „implizite Spuren sind unbegrenzt, also laeuft nichts um").
    //
    // GEMESSEN, dass die alte Begruendung und die neue dieselbe Zahl
    // liefern: `l4_spaltenfluss_braucht_die_zeilenvorlage` behauptet
    // `(0, 1, 5)` fuer Spalte und Zeilen — dieselben wie 11:3x.
    if spaltenfluss {
        // Release 08.10.2026: CSS Grid 1 §8.5 („Determine the rows in the
        // implicit grid"): ohne `grid-template-rows` hat das Raster EINE
        // Zeile, und jedes Kind oeffnet eine neue Spalte. GEMESSEN vorher:
        // t-online.de, die Hauptnavigation `grid grid-flow-col` stand
        // senkrecht (ein Eintrag je Zeile, alle 900 px breit).
        let zeilen_je_spalte = zeilen.len().max(1);
        // Die impliziten Spalten sind `auto` (§7.3): so breit wie ihr
        // breitester Inhalt — oder `grid-auto-columns`, wenn fest.
        let ncol_e = if vorlage_leer { 0 } else { ncol };
        let anz_spalten = n.div_ceil(zeilen_je_spalte).max(ncol_e);
        let mut spalten_breite: Vec<f32> = Vec::with_capacity(anz_spalten);
        let mut spalten_x: Vec<f32> = Vec::with_capacity(anz_spalten);
        // Release 09.10.2026: implizite Spalten nach `grid-auto-columns`
        // (§7.3) MIT `fr` und `auto`: zuerst Fest/Prozent/Inhalt, dann der
        // freie Platz — an die `fr`-Spalten nach Anteil (§7.2.4), sonst
        // gleichmaessig an die `auto`-Spalten (§12.8, `justify-content:
        // normal` dehnt `auto`-Spuren). GEMESSEN vorher: heise.de,
        // `grid-auto-flow: column; grid-auto-columns: minmax(0,1fr)` —
        // vier Kacheln so schmal wie ihr Text statt je ein Viertel.
        let mut anteil: Vec<f32> = Vec::with_capacity(anz_spalten);
        let mut dehnbar: Vec<bool> = Vec::with_capacity(anz_spalten);
        let mut fr_min: Vec<f32> = Vec::with_capacity(anz_spalten);
        for sp in 0..anz_spalten {
            let mut mindest = 0.0f32;
            let (b, fr, auto) = if sp < ncol_e {
                (breiten[sp.min(31)], 0.0, false)
            } else {
                let inhalt = kinder
                    .iter()
                    .take(n)
                    .enumerate()
                    .filter(|(i, _)| i / zeilen_je_spalte == sp)
                    .map(|(_, k)| sauber_pos(k.breit))
                    .fold(0.0f32, f32::max)
                    .min(c.max(0.0));
                match auto_spalten.get((sp - ncol_e) % auto_spalten.len().max(1)) {
                    Some(SpaltenMass::Fest(v)) => (sauber_pos(*v), 0.0, false),
                    Some(SpaltenMass::Prozent(p)) if c > 0.0 => (sauber_pos(*p * c), 0.0, false),
                    Some(SpaltenMass::Anteil(f)) if *f > 0.0 && c > 0.0 => (0.0, *f, false),
                    // `minmax(min, Nfr)`: ein `fr`-Anteil mit Untergrenze.
                    Some(SpaltenMass::MinMax { min, max }) if c > 0.0 => match max.as_ref() {
                        SpaltenMass::Anteil(f) if *f > 0.0 => {
                            mindest = sauber_pos(*min);
                            (0.0, *f, false)
                        }
                        _ => (inhalt.max(sauber_pos(*min)), 0.0, true),
                    },
                    _ => (inhalt, 0.0, true),
                }
            };
            spalten_breite.push(b);
            anteil.push(fr);
            dehnbar.push(auto);
            fr_min.push(mindest);
        }
        if c > 0.0 && anz_spalten > 0 {
            let luecken = gap * (anz_spalten.saturating_sub(1)) as f32;
            let belegt: f32 = spalten_breite.iter().sum();
            let frei = (c - luecken - belegt).max(0.0);
            let summe_fr: f32 = anteil.iter().sum();
            if summe_fr > 0.0 {
                for ((b, f), m) in spalten_breite.iter_mut().zip(anteil.iter()).zip(fr_min.iter()) {
                    if *f > 0.0 {
                        *b = sauber_pos((frei * *f / summe_fr.max(1.0)).max(*m));
                    }
                }
            } else {
                let n_auto = dehnbar.iter().filter(|d| **d).count();
                if n_auto > 0 && frei > 0.0 {
                    let zu = frei / n_auto as f32;
                    for (b, d) in spalten_breite.iter_mut().zip(dehnbar.iter()) {
                        if *d {
                            *b = sauber_pos(*b + zu);
                        }
                    }
                }
            }
        }
        let mut x_lauf = 0.0f32;
        for (sp, b) in spalten_breite.iter().enumerate() {
            let x = if sp < ncol_e { xpos[sp.min(31)] } else { x_lauf };
            spalten_x.push(x);
            x_lauf = x + b + gap;
        }
        // GEMESSENE ENTSCHEIDUNG, warum die implizite Spalte NICHT
        // 0 px breit ist: §7.3 gibt ihr `grid-auto-columns`, dessen
        // Anfangswert `auto` ist, und `auto` als Spur-Hoechstwert ist
        // `max-content` (CSS Sizing 3 §5.2.1) — den koennen wir hier
        // nicht messen, weil `RasterKind` nur die Hoehe traegt. Eine
        // Breite von 0 waere ein Loch im Bild, also nehmen wir die
        // BREITESTE EXPLIZITE Spur als untere Schranke: sichtbar, und
        // nie breiter als der Container. Steht als eigene Funktion, weil
        // eine erfundene Zahl an zwei Stellen nicht dieselbe bleibt.
        // L4 (02.10.2026 21:6x): `grid-auto-columns` (§7.3) ZUERST, und
        // erst sonst die untere Schranke aus 21:4x.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Liste ZYKLISCH gelesen wird:
        // §7.5 nennt das Wiederholen fuer `grid-auto-rows` ausdruecklich
        // ("the pattern is repeated as necessary"), und §7.3 meint es
        // fuer die Spalten genauso. Eine Liste aus EINER Spur gilt also
        // fuer alle impliziten Spalten; sonst waere
        // `grid-auto-columns: 200px` bei fuenf impliziten Spalten eine
        // Angabe fuer eine.
        //
        // GEMESSENE ENTSCHEIDUNG, warum nur `Fest` gelesen wird: eine
        // implizite Spalte ist nach §7.3 `auto`, und `auto` ist
        // `max-content` — nicht messbar hier (`RasterKind` traegt nur die
        // Hoehe). `fr` braucht eine feste Containerbreite auf der
        // Querachse, die es nicht gibt. Also faellt die Schranke.
        let auto_breite = |k: usize| -> Option<f32> {
            if auto_spalten.is_empty() {
                return None;
            }
            match auto_spalten[k % auto_spalten.len()] {
                SpaltenMass::Fest(v) => Some(sauber_pos(v)),
                _ => None,
            }
        };
        let implizit = breiten
            .iter()
            .take(ncol)
            .copied()
            .fold(0.0f32, f32::max)
            .max(breiten[0]);
        // Die y-Position je Zeile VORAB rechnen: im Spaltenfluss laeuft
        // y nicht durch, jede Zeile hat ihre eigene Position.
        let mut ypos: [f32; 32] = [0.0; 32];
        let mut yy = 0.0f32;
        for z in 0..zeilen_je_spalte.min(32) {
            ypos[z] = yy;
            // Die Zeilenhoehe haengt davon ab, WELCHE Kinder in der Zeile
            // liegen — das ist die Zeile 0 oder 1 der NAECHSTEN Spalte.
            // Deshalb wird sie hier aus allen Kindern dieser Zeilenlage
            // genommen; das ist §8.3 (eine Zeile hat eine Spur fuer alle
            // ihre Spalten).
            let mut h = 0.0f32;
            for (i, k) in kinder.iter().take(n).enumerate() {
                if i % zeilen_je_spalte == z {
                    h = h.max(auto_zeilen_hoehe(z, zeilen, auto_zeilen, k.hoch));
                }
            }
            yy += h + gap_zeile;
        }
        for (i, k) in kinder.iter().take(n).enumerate() {
            let zz = i % zeilen_je_spalte;
            let sp = i / zeilen_je_spalte;
            // Release 08.10.2026: Breite und Lage aus der Spaltenrechnung
            // oben (explizit aus der Vorlage, implizit aus dem Inhalt).
            let breite = spalten_breite.get(sp).copied().unwrap_or_else(|| auto_breite(sp).unwrap_or(implizit));
            let xx = spalten_x.get(sp).copied().unwrap_or_else(|| implizit_x(sp, ncol, &breiten, gap, implizit));
            // L4 (09:5x): §6.6 — das Kind wird in seiner Zelle
            // ausgerichtet (die Funktion traegt die Begruendung).
            let (xx, yy, bb, hh) = richte_kind(
                k,
                xx,
                ypos[zz],
                breite,
                auto_zeilen_hoehe(zz, zeilen, auto_zeilen, k.hoch),
            );
            zellen.push(RasterZelle {
                x: xx,
                y: yy,
                breite: bb,
                hoehe: hh,
                zeile: zz,
                spalte: sp,
            });
        }
        return zellen;
    }
    // L4 (02.10.2026 22:4x): §8.3 UND §8.5 — DER PLATZIERUNGS-CURSOR.
    //
    // GEMESSENER BEFUND, was hier vorher stand: `zeile = idx / ncol`
    // und dann `ncol` Items hintereinander. Jedes Item bekam genau eine
    // Zeile, und seine Hoehe war das groesste Kind SEINER Zeile. Also
    // waren `grid-row: span N` und `grid-column: span N` wirkungslos.
    //
    // GEMESSENE ENTSCHEIDUNG, warum es eine eigene Zellen-Matrix ist und
    // nicht ein Index pro Item: §8.5 setzt voraus, dass die belegten
    // Zellen bekannt sind („the placement cursor skips over cells occupied
    // by other grid items"). Bei einem Index pro Item muesste man fuer
    // jedes Item alle vorherigen durchsuchen — bei `n` Items `n^2`, und
    // die Reihenfolge der Items waere die Reihenfolge der Pruefung. Eine
    // Matrix ist `n` Speicher und ein Durchgang je Platzierung.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Groesse der Matrix `n*n` ist und
    // nicht `n*ncol`: `n` Items koennen hoeher stehen als die Vorlage
    // (implizite Spuren, §7.3/§7.5), und die Grenze steht in `n`. Der
    // Deckel ist `MAX_RASTER_ZELLEN`, wie bei der Anzahl der Kinder.
    //
    // GEMESSENE ENTSCHEIDUNG, warum in ZWEI Schritten gearbeitet wird
    // (erst die Lage, dann die Groessen) — und das ist der zweite eigene
    // Fehler dieses Punktes: ich habe zuerst die Zeilenhoehen aus ALLE
    // Kindern gebildet und dann platziert. GEMESSEN hat es
    // `c15_zeilenhoehe_ist_das_zeilenmaximum` (`left: 30.0` statt `5.0`)
    // und `l4_fr_auf_der_zeilenachse` (`left: (70.0, 70.0)` statt
    // `(70.0, 30.0)`): die Zeile 1 bekam das groesste Kind des RASTERS,
    // nicht das ihrer eigenen Zeile. §8.3 rechnet eine Zeile aus den
    // Kindern, die in ihr stehen — und wer das sind, weiss man erst
    // nach der Platzierung.
    //
    // L4 (02.10.2026 23:5x): DIE ZEILENZAHL IST NICHT `n`. GEMESSENER
    // BEFUND, gefunden ueber den Seitenweg (`examples/l4_raster.rs`,
    // Block O2): 4 Kinder, davon eins ueber 2 Zeilen, meldeten
    // **3 Kinder**. Ursache: `n` Kinder brauchen mit Spannen bis zu
    // `summe(span)` Zeilen, und die Matrix/`hoehen` waren auf `n`
    // gebaut. Das vierte Kind bekam die Zeile 4, `hoehen[4]` gibt es
    // nicht, seine Hoehe wurde 0 — und die Messform blendet Kaeste mit
    // Hoehe 0 aus, also war das Kind **weg**.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Grenze die SUMME der Spannen ist
    // und nicht `n * MAX_SPAN`: die Summe ist die tatsaechliche
    // Platzierung (jedes Kind belegt so viele Zeilen, wie es beansprucht),
    // und sie ist durch `MAX_RASTER_ZELLEN` gedeckelt. `n * 65535` waere
    // eine Zahl, die man nur begrenzen kann.
    let zeilen_bedarf: usize = kinder.iter().take(n).map(|k| k.spann_zeilen.max(1)).sum();
    // GEMESSENE ENTSCHEIDUNG, warum es fuer die SPALTEN keine
    // Bedarfsrechnung gibt (23:5x): GEMESSEN war
    // `[(0,0), (1,1), (0,2)]` — bei einem Raster mit EINER Vorlagenspalte
    // legte das zweite Kind in eine implizite Spalte. §8.5 sagt aber: „If
    // the cursor is already at the end of the row, increment the row
    // position by 1 and reset the column position to the start of the
    // row". Das Ende der Zeile ist die VORLAGE (`ncol`), nicht das, was
    // ein Kind schon erzeugt hat. Zeilen werden implizit (§7.3/§7.5),
    // Spalten im Zeilenfluss nicht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `spann_spalten` trotzdem gelesen
    // wird, obwohl es keine Zeilen erzeugt: es bestimmt die BREITE des
    // Kindes (Schritt 3), nicht seine Lage.
    //
    // L4 (03.10.2026 01:4x): DIE FESTEN LINIEN SETZEN DIE ZEILENZAHL.
    //
    // GEMESSENER FEHLER MEINER ERSTEN FASSUNG, gefunden von
    // `l4_feste_startlinie_setzt_zeilen_und_spalten`: `max_zeilen` kam
    // aus der KINDERzahl, also war sie bei EINEM Kind gleich 1. `passt`
    // lehnt eine Area ab, die aus der Matrix herausragt, also wurde die
    // feste Zeile 3 abgelehnt, der Cursor lief ans Ende und das Kind
    // landete in (0, 2) statt in (2, 1).
    //
    // GEMESSENE ENTSCHEIDUNG, warum nur POSITIVE Linien in die Grenze
    // eingehen: `zeile_start: 3` heisst Index 2 unabhaengig von der
    // Zeilenzahl. Negative Linien (`-1`) brauchen die Zeilenzahl — und
    // werden in `linie()` danach aufgeloest, also zu spaet fuer diese
    // Grenze. §7.3/§7.5: implizite Spuren entstehen „as many as
    // necessary", und eine feste Linie ist genau eine solche Notwendigkeit.
    let pos_zeilen: usize = kinder
        .iter()
        .take(n)
        .map(|k| k.zeile_start.max(0) as usize)
        .max()
        .unwrap_or(0);
    let max_zeilen = n
        .max(zeilen_bedarf)
        .max(pos_zeilen)
        .min(MAX_RASTER_ZELLEN)
        .max(1);
    // L4 (03.10.2026 00:6x): DIE SPANNENZAHL BESTIMMT DIE ZEILENBREITE.
    //
    // GEMESSENER BEFUND, gefunden von `l4_lage_bei_spaltenspannen` (4
    // Spalten a 100 px, alle Kinder `grid-column: auto/span 3`):
    //
    //     LAGE [(0,0), (0,3), (1,2), (2,1)]
    //
    // Kind 1 steht in Spalte 3 und markiert `(0,3)`, `(0,4)` und
    // `(0,5)` — und mit dem Zeilenabstand 4 sind `(0,4)` und `(0,5)`
    // die Zellen **(1,0)** und **(1,1)**. Die Spannweite lief in die
    // naechste Zeile und hat dort zwei Zellen belegt, die frei waren.
    // Deshalb stand Kind 2 nicht am Anfang von Zeile 1, sondern in
    // Spalte 2.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `max(ncol, groesste Spannweite)` und
    // nicht `ncol`: §7.3 sagt, dass implizite Spuren erzeugt werden „as
    // needed", und `grid-column: auto/span 12` (362 Vorkommen im
    // Korpus, GEMESSEN) braucht zwoelf Spalten, von denen acht implizit
    // sind. **Und** §8.5 sagt, dass der Cursor rueckt, BIS die ganze
    // Grid-Area frei ist — also passt eine Spanne nur, wenn sie in die
    // Zeile passt. Beides zusammen ergibt: die Zeile ist so breit wie
    // `ncol`, ausser ein Kind verlangt mehr.
    let spalten_max = kinder
        .iter()
        .take(n)
        .map(|k| k.spann_spalten.max(1))
        .max()
        .unwrap_or(1);
    let pos_spalten: usize = kinder
        .iter()
        .take(n)
        .map(|k| k.spalte_start.max(0) as usize)
        .max()
        .unwrap_or(0);
    // L4 (04:13x): die ENDLINIE der Spalten zaehlt mit (siehe die
    // Begruendung bei `max_zeilen`).
    let mut pos_spalten_max = pos_spalten;
    //
    // L4 (03.10.2026 01:5x): NEGATIVE LINIEN ZAEHLEN AB DER VORLAGE.
    //
    // GEMESSENER BEFUND, gefunden von `l4_negative_linie_zeigt_auf_die_
    // letzte`: `grid-row-start: -1` in einem Raster mit
    // `grid-template-rows: 50px ×4` und zwei Kindern landete in Zeile 1
    // statt in Zeile 4. Ursache: `max_zeilen` kam aus der KINDERzahl (2),
    // und §8.3 sagt „A negative integer refers to a line counted backwards
    // from the end of the **explicit** grid". Die Vorlage hat vier
    // Zeilen, also ist `-1` die Linie dahinter — Index 4.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Vorlage und nicht die Matrix:
    // die Matrix ist unser Rechenraum, nicht das Raster. Eine negative
    // Linie beschreibt das Raster, und das ist die Vorlage plus die
    // impliziten Spuren — von denen man die Linie nicht abzaehlen kann,
    // weil sie erst durch die Linien entstehen.
    let ncol_eff_vorlage = ncol;
    let ncol_eff = ncol
        .max(spalten_max)
        .max(pos_spalten_max)
        .min(MAX_RASTER_SPALTEN)
        .max(1);
    let _ = ncol_eff_vorlage;
    // L4 (01:5x): die Grenze traegt die AUFGELOESTEN Linien. Eine feste
    // Zeile 5 in einem Raster mit zwei Kindern braucht vier Zeilen, sonst
    // lehnt `passt` sie ab (GEMESSEN: Zeile 0 statt 4).
    let mut max_zeilen = max_zeilen;
    for k in kinder.iter().take(n) {
        // L4 (03.10.2026 04:13x): DIE ENDLINIE ZAEHLT MIT. GEMESSENER
        // BEFUND (Block T5, Release, `grid-template-rows: 50px ×6`,
        // `grid-area: 1/1/-1/-1`): die Item-Hoehe war **50** statt 300 und
        // das Item stand in y 98 statt 48. Grund: `passt` lehnt eine
        // Grid-Area ab, die aus der Matrix herausragt, und die Grenze
        // kannte nur die STARTlinien — die Endlinie `-1` (Index 6) wurde
        // nicht mitgezaehlt, also war `max_zeilen` 2 statt 7.
        //
        // GEMESSENE ENTSCHEIDUNG, warum es derselbe Fehler wie von 01:5x
        // ist und nicht ein neuer: dort fehlte der Platz HINTER der
        // letzten Startlinie, hier der Platz BIS zur Endlinie. Die
        // Grenze brauchte in beiden Faellen die LINIE, nicht das Item.
        if let Some(z) = linie(k.zeile_ende, zeilen.len()) {
            max_zeilen = max_zeilen.max(z + 1);
        }
        if let Some(z) = linie(k.spalte_ende, ncol) {
            pos_spalten_max = pos_spalten_max.max(z + 1);
        }
        if let Some(z) = linie(k.zeile_start, zeilen.len()) {
            // `+ 1` GEMESSEN noetig (01:5x): ohne die eine Zeile standen
            // zwei Kinder in derselben Zeile — GEMESSEN `(4, 4)` statt
            // `(4, 5)`. Der Cursor bricht am Ende der Matrix ab und
            // klemmt auf die letzte Zeile, also braucht es hinter der
            // letzten belegten Zeile einen freien Platz. §7.5: „as many
            // implicit tracks as necessary".
            max_zeilen = max_zeilen.max(z + k.spann_zeilen.max(1) + 1);
        }
    }
    // Release 11.10.2026: das Raster hat ALLE Zeilen der Vorlage, auch leere
    // (CSS Grid 1 §7.1) — sie zaehlen zur Hoehe und tragen den Rasterbereich
    // positionierter Elemente.
    let max_zeilen = max_zeilen.max(zeilen.len()).min(MAX_RASTER_ZELLEN).max(1);
    let grenze = max_zeilen.saturating_mul(ncol_eff).min(MAX_RASTER_ZELLEN);
    let mut belegt = vec![false; grenze.max(1)];
    // Schritt 1: die Lage jedes Items (Zeile, Spalte, Spannweite).
    //
    // §8.5, und der Cursor wird ueber ALLE Items gefuehrt. GEMESSENER
    // BEFEUND, warum das der Kern ist: mit `zeile = idx / ncol_eff` neu
    // berechnet zu werden war `grid-column: 2` an einem Kind wirkungslos,
    // sobald ein vorheriges Kind eine feste ZEILE hatte — GEMESSEN:
    // `[(0,0), (2,0), (0,1)]` statt `[(0,0), (2,0), (2,1)]`. §8.5 fuehrt
    // den Cursor ueber die Items, und „the previous item's column-start
    // line" setzt genau das voraus.
    let mut lage: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(n);
    let mut cur_zeile = 0usize;
    let mut cur_spalte = 0usize;
    let mut vor_zeile: Option<usize> = None;
    let mut vor_spalte: Option<usize> = None;
    for k in kinder.iter().take(n) {
        // §8.4: ein benanntes Gebiet gibt die Spannweite mit — es ist
        // „the smallest rectangle that contains all the cells with that
        // name". Also die Zahl der Zeilen/Spalten des Rechtecks, nicht
        // `span N` aus der Angabe.
        //
        // GEMESSENER EIGENER FEHLER (02:14x), gefunden von
        // `l4_grid_area_legt_das_kind_in_sein_rechteck`: das Rechteck lag in
        // einem Tupel aus vier `usize` OHNE Namen, und ich hatte `t.2` fuer
        // die SPALTE gehalten — es war die Zeilen-ENDgrenze. GEMESSEN:
        // `left: (1, 1)` statt `(1, 0)`. Darum ist es jetzt eine Struktur
        // mit benannten Feldern: vier gleiche Zahlen sind an der
        // Benutzungsstelle nicht lesbar.
        // L4 (03.10.2026 06:2x): **die beiden BLOECKE MIT `span` SIND RAUS**, und
        // mit ihnen das `im_gebiet` darueber — es wurde weiter unten neu
        // gebildet, und die alte Fassung wurde von niemandem gelesen.
        //
        // Um 04:12x stand hier
        //
        //     let span = match im_gebiet { Some(t) => t.hoehe, None => k.spann_zeilen.max(1) };
        //     let spann = match im_gebiet { Some(t) => t.breite, None => k.spann_spalten.max(1) };
        //
        // und wurde durch die ENDLINIEN-Fassung weiter unten (1220ff)
        // ersetzt. Die beiden Fassungen sind nicht nebeneinander: der obere
        // Block war die Fassung OHNE Endlinie.
        //
        // GEMESSENER BEFUND, warum er bis 06:2x stand und nicht auffiel:
        // `bau.sh ... clippy` meldet GRUEN, weil clippy ohne
        // `-D warnings` laeuft und eine Warnung den Exitcode NICHT
        // aendert. Die Warnung sah erst `bau.sh narsil-css check --tests`.
        // **Dieselbe Zahl an zwei Stellen einer Platzierung ist genau die
        // Buchung, die auseinanderlaufen kann.**
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Befund als Kommentar bleibt
        // und nicht einfach verschwindet: wer `let span` sucht, findet die
        // Begruendung, warum es nur noch eine gibt.
        // L4 (03.10.2026 02:12x): DAS BENANNTE GEBIET (§8.4) hat VORRANG.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das VOR der Linie kommt: §8.4 sagt
        // „If the placement contains a named grid area, ... the item's
        // grid area is the named area". Also gewinnt der Name, und die
        // LINIEN sind dann der Fall „kein Name".
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Linien auch dann gesetzt
        // werden, wenn ein Name da ist: `grid-area: visual` ist ein Name,
        // aber `grid-row: 2` daneben ist eine Zeile — und §8.4 bestimmt
        // den Vorrang nicht ausdruecklich. Wir nehmen die Spec beim Wort:
        // „the item's grid area IS the named area".
        let im_gebiet = gebiets_rechtecke
            .iter()
            .find(|t| t.name == k.gebiet && !k.gebiet.ist_leer());
        // =================================================================
        // §8.4: DAS BENANNTE GEBIET IST EINE FESTE LAGE (06.10.2026)
        // =================================================================
        //
        // GEMESSENER FEHLER AM LEBENDEN BEISPIEL (de.wikipedia.org,
        // 1191 px): `.mw-body` hat die Bereiche
        //     'titlebar-cx .' 'titlebar columnEnd'
        //     'toolbar columnEnd' 'content columnEnd'.
        // GEMESSEN stand `content` bei x=995 (NEBENspalte) statt bei 308,
        // und `columnEnd` bei x=308 statt 995 — sie waren VERTAUSCHT.
        //
        // GEMESSENE URSACHE: die alte Fassung setzte aus dem Gebiet nur
        // `fest_zeile`/`fest_spalte` und liess danach den Suchlauf des
        // Auto-Flows (§8.5) laufen. Der Cursor stand wegen der
        // „vorherigen Zeile/Spalte" auf einer belegten Zelle — bei
        // `columnEnd` war die Zielzelle (1,1) durch die Suchregel
        // unerreichbar, er landete auf (3,0), und `content` fand seinen
        // Platz (3,0) belegt und wich nach (3,1) aus. Damit lag der
        // ganze Artikeltext hinter dem Fensterrand.
        //
        // GEMESSENE REGEL: §8.4 sagt „the item's grid area IS the named
        // area" — Zeile UND Spalte sind bestimmt, und §8.5 sucht nur,
        // wenn eine Achse `auto` ist. Ein Gebietskind wird deshalb
        // GESETZT, nicht gesucht.
        if let Some(t) = im_gebiet {
            let zeile = t.zeile.min(max_zeilen.saturating_sub(1));
            let spalte = t.spalte.min(ncol_eff.saturating_sub(1));
            let span = t.hoehe.max(1);
            let spann = t.breite.max(1);
            for dz in 0..span {
                for ds in 0..spann {
                    if let Some(p) = feld(zeile + dz, spalte + ds, ncol_eff)
                        && p < belegt.len()
                    {
                        belegt[p] = true;
                    }
                }
            }
            lage.push((zeile, spalte, span, spann));
            vor_zeile = Some(zeile);
            vor_spalte = Some(spalte);
            cur_spalte = spalte.saturating_add(spann);
            cur_zeile = zeile;
            if cur_spalte >= ncol_eff {
                cur_spalte = 0;
                cur_zeile = zeile.saturating_add(span);
            }
            continue;
        }
        let fest_zeile = match im_gebiet {
            Some(t) => Some(t.zeile),
            None => linie(k.zeile_start, zeilen.len()),
        };
        let fest_spalte = match im_gebiet {
            Some(t) => Some(t.spalte.min(ncol_eff.saturating_sub(1))),
            None => linie(k.spalte_start, ncol),
        };
        // L4 (03.10.2026 04:12x): §8.3 — DIE ENDLINIE BESTIMMT DIE
        // SPANNWEITE, und eine negative Endlinie zae vom ENDE der
        // VORLAGE.
        //
        // GEMESSENER BEFUND, warum das hier und nicht in
        // `layout_plan::spanne()`: `spanne()` kennt `Berechnet`, und
        // `Berechnet` kennt nur die Vorlage des KNOTENS — die Vorlage
        // steht aber am CONTAINER und erbt nicht (§8.1/§8.3 machen
        // `grid-template-rows` nicht erblich). GEMESSEN (Block T5,
        // Release, `grid-template-rows: 50px ×6`, `grid-area: 1/1/-1/-1`):
        // **h 50** statt 250, weil `raster_zeilen` des Kindes leer war.
        // Ich hatte die Aufloesung in `layout_plan` gebaut und wieder
        // weggenommen — der Befund steht dort als Kommentar.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Endlinie nur dann rechnet,
        // wenn keine `span`-Angabe da ist: §8.3 gibt der `span`-Form den
        // Vorrang („the grid item spans that many tracks"), und der
        // `RasterKind` traegt die Spannweite bereits aufgeloest.
        let ende_zeile = if k.zeile_ende != 0 && k.spann_zeilen.max(1) == 1 {
            Some(linie(k.zeile_ende, zeilen.len()).unwrap_or(1))
        } else {
            None
        };
        let ende_spalte = if k.spalte_ende != 0 && k.spann_spalten.max(1) == 1 {
            Some(linie(k.spalte_ende, ncol).unwrap_or(1))
        } else {
            None
        };
        let span = match (im_gebiet, ende_zeile) {
            (Some(t), _) => t.hoehe,
            (None, Some(e)) => e.saturating_sub(fest_zeile.unwrap_or(0)).max(1),
            (None, None) => k.spann_zeilen.max(1),
        };
        let spann = match (im_gebiet, ende_spalte) {
            (Some(t), _) => t.breite,
            (None, Some(e)) => e.saturating_sub(fest_spalte.unwrap_or(0)).max(1),
            (None, None) => k.spann_spalten.max(1),
        };
        // Die Linien erst HIER aufloesen (§7.3/§7.5: die Zahl der Spuren
        // steht erst nach dem Zaehlen fest) — siehe `linie`.
        // GEMESSENE ENTSCHEIDUNG (01:5x): aufgeloest wird gegen die
        // VORLAGE (`zeilen.len()`, `ncol`), nicht gegen die interne
        // Grenze — siehe die Begruendung bei `ncol_eff`.
        // L4 (03.10.2026 04:2x): `dense` (§8.5) — DIE LOECHER FUELLEN.
        //
        // §8.5: „in the dense packing mode, for each grid item, ... the
        // CSS grid auto-placement algorithm instead places it in the next
        // EMPTY area". GEMESSENE ENTSCHEIDUNG, warum das ein ZURUECKSETZEN
        // des Cursors ist und nicht eine eigene Suchschleife: „next empty
        // area" heisst: die Suche beginnt vorn. Und §8.5 nennt fuer
        // `dense` KEINEN eigenen Zweig fuer feste Positionen — die
        // Loeschung gilt nur fuer den Auto-Fluss, also genau dort, wo der
        // Cursor laeuft.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die LINIEN trotzdem VOR dem
        // Zuruecksetzen kommen: `grid-row: 3` setzt die Zeile, und
        // §8.5s Festlegungs-Zweig gilt fuer beide Packungen. Also erst
        // die Linien, dann — falls `dense` — der Cursor auf vorn.
        if dicht {
            cur_zeile = 0;
            cur_spalte = 0;
        }
        // §8.5, Zweig 1 („If the item has a definite grid position"):
        // erst die Spalte, dann — falls kleiner als die vorige — die
        // Zeile.  Zweig 2 („if the item has a definite row position"):
        // erst die Zeile, dann — falls kleiner als die vorige — die
        // Spalte.
        if let Some(fs) = fest_spalte {
            if Some(fs) < vor_spalte {
                cur_zeile = cur_zeile.saturating_add(1);
            }
            cur_spalte = fs;
        }
        if let Some(fz) = fest_zeile {
            if Some(fz) < vor_zeile {
                cur_spalte = cur_spalte.saturating_add(1);
            }
            cur_zeile = fz;
        }
        // §8.5 Schritt 2 und 3: „increment the cursor's column position
        // until it either finds a grid area that is not occupied ... or the
        // cursor reaches the end of the row (creating implicit rows as
        // necessary)" und danach dasselbe fuer die Zeile.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das EINE Schleife fuer beide
        // Achsen ist: §8.5 nennt zwei Schritte, aber sie haben dieselbe
        // Bedingung und dieselbe Wirkung (eine Zelle weiter, Umbruch am
        // Ende). Eine Schleife, die die Achse selbst waehlt, waere eine
        // zweite Buchung derselben Regel.
        while !passt(
            &belegt, cur_zeile, cur_spalte, ncol_eff, max_zeilen, span, spann,
        ) {
            cur_spalte += 1;
            if cur_spalte + spann > ncol_eff {
                cur_spalte = 0;
                cur_zeile = cur_zeile.saturating_add(1);
            }
            if cur_zeile >= max_zeilen {
                // §8.5: „creating implicit rows as necessary" — die Matrix
                // ist gedeckelt, also endet die Suche.  GEMESSENE
                // ENTSCHEIDUNG, warum hier KEINE Abbruchbedingung im
                // Normalfall steht: `passt` ist bei `cur_zeile <
                // max_zeilen` immer irgendwann wahr, weil die Zeile nach
                // einem leeren Durchlauf frei ist.
                break;
            }
        }
        let zeile = cur_zeile.min(max_zeilen.saturating_sub(1));
        let spalte = cur_spalte.min(ncol_eff.saturating_sub(1));
        // §8.5: die belegten Zellen markieren — inklusive der SPANNEN.
        for dz in 0..span {
            for ds in 0..spann {
                if let Some(p) = feld(zeile + dz, spalte + ds, ncol_eff)
                    && p < belegt.len()
                {
                    belegt[p] = true;
                }
            }
        }
        // Der Cursor rueckt HINTER das Item.
        //
        // GEMESSENER FEHLER MEINER ERSTEN FASSUNG, gefunden von
        // `c15_feste_spalten_mit_zahlen`: ich sprang nach jeder Platzierung
        // eine Zeile tiefer (`cur_zeile = zeile + span`). GEMESSEN war
        // `z[0].hoehe = 30` statt 40 — die zwei Kinder standen in zwei
        // Zeilen, und die Zeile 0 hatte nur das 30-px-Kind.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Cursor im Zeilenfluss erst
        // nach RECHTS laeuft: §8.5 Schritt 2 sagt „increment the cursor's
        // column position until it ... reaches the end of the row", und
        // erst dann entsteht die naechste Zeile. Der Zeilenfluss ist der
        // Spaltenfluss mit vertauschten Achsen — und der Spaltenfluss
        // steht in einem eigenen Zweig weiter oben.
        cur_spalte = spalte.saturating_add(spann);
        cur_zeile = zeile;
        if cur_spalte >= ncol_eff {
            cur_spalte = 0;
            cur_zeile = zeile.saturating_add(span);
        }
        // §8.5: „the previous item's row-start line" — GEMESSENE
        // ENTSCHEIDUNG, warum bei einem Item OHNE feste Linie die
        // PLATZIERUNG gilt und nicht `auto`: der Vergleich in §8.5
        // bezieht sich auf den zuletzt benutzten Cursorwert, und der ist
        // nach der Platzierung die gelegte Zelle. Sonst waere
        // `grid-column: 2` an allen Kindern identisch — jede in Zeile 0.
        vor_zeile = Some(zeile);
        vor_spalte = Some(spalte);
        lage.push((zeile, spalte, span, spann));
    }
    // Schritt 2: die Zeilenhoehe aus den Kindern, die IN der Zeile stehen
    // (§8.3: `auto` ist der Inhalt, eine Laenge ist die Spur).
    //
    // L4 (03.10.2026 01:8x): EINE LEERE ZEILE HAT IHRE SPURGROESSE.
    //
    // GEMESSENER BEFEUND, gefunden von `l4_zeile_dreier_kind_zweiter_`:
    // `LAGE [(0,0,0.0), (2,1,50.0), (2,2,50.0), (2,3,50.0)]` — die Zeile
    // war richtig (Index 2), aber `y` war 50 statt 148. Ursache: `hoehen`
    // kam aus den Kindern, und Zeile 1 war LEER — also 0 px, und die
    // y-Positionen rutschten auf.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Anfangswert aus der Spur kommt und
    // nicht aus einem Kind: §8.3 macht die Spur zum MASS der Zeile. Eine
    // Zeile ohne Kinder ist nicht 0 hoch — sie ist so hoch wie ihre Spur.
    // Das ist genau der Unterschied zu `auto`: `auto` heisst Inhalt, und
    // ein leerer Inhalt ist 0.
    let mut hoehen: Vec<f32> = (0..max_zeilen)
        .map(|r| auto_zeilen_hoehe(r, zeilen, auto_zeilen, 0.0))
        .collect();
    for (i, &(zeile, _, span, _)) in lage.iter().enumerate() {
        for dz in 0..span {
            let r = zeile + dz;
            if r < hoehen.len() {
                hoehen[r] =
                    hoehen[r].max(auto_zeilen_hoehe(r, zeilen, auto_zeilen, kinder[i].hoch));
            }
        }
    }
    // Release 11.10.2026: leere `auto-fit`-Zeilen kollabieren (§7.2.3.2) —
    // Hoehe 0, und die Luecke daneben faellt weg.
    let mut zeile_kollabiert = vec![false; hoehen.len()];
    if zeilen_fit.iter().any(|f| *f) {
        let mut besetzt = vec![false; hoehen.len()];
        for &(zeile, _, span, _) in &lage {
            for dz in 0..span {
                if let Some(b) = besetzt.get_mut(zeile + dz) {
                    *b = true;
                }
            }
        }
        for r in 0..hoehen.len().min(zeilen_fit.len()) {
            if zeilen_fit[r] && !besetzt[r] {
                zeile_kollabiert[r] = true;
                hoehen[r] = 0.0;
            }
        }
    }
    // Release 11.10.2026: bei definiter Hoehe bekommen Prozentzeilen ihren
    // Anteil und `fr`-Zeilen den freien Platz (§7.2.4, §11.7 „Expand Flexible
    // Tracks", mit `minmax(x, Nfr)`; Zeilen, deren Inhalt groesser ist als ihr
    // Anteil, scheiden aus). GEMESSEN vorher: `grid-template-rows: auto 1fr
    // auto` in `height: 300px` (oder `min-height: 100vh`) — jede Zeile so
    // hoch wie ihr Inhalt, die Fusszeile stand mitten auf der Seite.
    if let Some(h) = zeilen_platz.filter(|h| h.is_finite() && *h > 0.0) {
        let spur = |r: usize| -> Option<&SpaltenMass> {
            if r < zeilen.len() {
                zeilen.get(r)
            } else if auto_zeilen.is_empty() {
                None
            } else {
                auto_zeilen.get((r - zeilen.len()) % auto_zeilen.len())
            }
        };
        for r in 0..hoehen.len() {
            if let Some(SpaltenMass::Prozent(p)) = spur(r)
                && !zeile_kollabiert[r]
            {
                hoehen[r] = sauber_pos(*p * h);
            }
        }
        let faktor = |r: usize| -> f32 {
            match spur(r) {
                Some(SpaltenMass::Anteil(f)) => sauber_pos(*f),
                Some(SpaltenMass::MinMax { max, .. }) => match max.as_ref() {
                    SpaltenMass::Anteil(f) => sauber_pos(*f),
                    _ => 0.0,
                },
                _ => 0.0,
            }
        };
        let mut flexibel: Vec<usize> = (0..hoehen.len()).filter(|&r| !zeile_kollabiert[r] && faktor(r) > 0.0).collect();
        let sichtbar = (0..hoehen.len()).filter(|&r| !zeile_kollabiert[r]).count();
        let luecken = gap_zeile * sichtbar.saturating_sub(1) as f32;
        for _ in 0..8 {
            if flexibel.is_empty() {
                break;
            }
            let fest: f32 = (0..hoehen.len())
                .filter(|r| !zeile_kollabiert[*r] && !flexibel.contains(r))
                .map(|r| hoehen[r])
                .sum();
            let summe: f32 = flexibel.iter().map(|&r| faktor(r)).sum();
            let frei = h - luecken - fest;
            if frei <= 0.0 || summe <= 0.0 {
                break;
            }
            let einheit = frei / summe.max(1.0);
            let zu_gross: Vec<usize> =
                flexibel.iter().copied().filter(|&r| hoehen[r] > einheit * faktor(r) + 0.01).collect();
            if zu_gross.is_empty() {
                for &r in &flexibel {
                    hoehen[r] = sauber_pos(einheit * faktor(r));
                }
                break;
            }
            flexibel.retain(|r| !zu_gross.contains(r));
        }
    }
    // Die y-Positionen (§8.1: `row-gap` steht zwischen zwei Zeilen, nicht
    // am Ende).
    let mut ypos = vec![0.0f32; max_zeilen];
    let mut y = 0.0f32;
    for r in 0..max_zeilen {
        ypos[r] = y;
        if zeile_kollabiert.get(r).copied().unwrap_or(false) {
            continue;
        }
        y += hoehen[r] + gap_zeile;
    }
    zeilen_spuren.extend((0..max_zeilen).map(|r| (ypos[r], hoehen[r])));
    // Schritt 3: die Zellen. §8.3: ein Item ueber mehrere Spuren ist so
    // gross wie die SUMME der Spuren (plus die Abstaende dazwischen).
    //
    // GEMESSEN, dass hier kein Index gebraucht wird (22:4x): Schritt 3
    // rechnet nur aus den SPUREN, nicht aus den Inhalten. Die Inhalte
    // haben die Zeilenhoehe in Schritt 2 bestimmt. Also `i` unbenutzt —
    // und GEMESSEN hat der Compiler das gemeldet.
    // L4 (09:5x): der Index kommt mit, weil die Ausrichtung das KIND
    // braucht und `lage` parallel zu `kinder` laeuft (siehe die Schleife
    // darüber, die `kinder[i]` liest).
    for (i, &(zeile, spalte, span, spann)) in lage.iter().enumerate() {
        let mut h = 0.0f32;
        for dz in 0..span {
            let r = zeile + dz;
            if r < hoehen.len() {
                h += hoehen[r];
            }
        }
        h += span.saturating_sub(1) as f32 * gap_zeile;
        let mut breite = 0.0f32;
        for ds in 0..spann {
            let sc = spalte + ds;
            if sc < ncol {
                breite += breiten[sc];
                if ds + 1 < spann {
                    breite += gap;
                }
            }
        }
        // L4 (09:5x): §6.6 — auch auf der CURSOR-Bahn wird das Kind
        // ausgerichtet; `k` ist das Kind dieser Runde (siehe die
        // Schleife ueber `kinder`).
        let kind = kinder.get(i).copied().unwrap_or_default();
        let (xx, yy, bb, hh) = richte_kind(
            &kind,
            xpos[spalte.min(ncol.saturating_sub(1))],
            ypos[zeile.min(ypos.len().saturating_sub(1))],
            sauber_pos(breite),
            sauber_pos(h),
        );
        zellen.push(RasterZelle {
            x: xx,
            y: yy,
            breite: bb,
            hoehe: hh,
            zeile,
            spalte,
        });
    }
    zellen
}

/// Die x-Position einer IMPLIZITEN Spalte (CSS Grid 1 §7.3).
///
/// L4 (02.10.2026 21:2x). GEMESSENE ENTSCHEIDUNG, warum es ueberhaupt
///_implizite_ Spalten gibt: mit einer Zeilenvorlage und mehr Kindern als
/// `Spalten * Zeilen` braucht §8.5 Spalten, die nicht in der Vorlage
/// stehen. GEMESSEN in `l4_spaltenfluss_braucht_die_zeilenvorlage`:
/// 2 Spalten, 2 Zeilen, 6 Kinder — die Kinder 5 und 6 liegen auf Spalte
/// 2, und die steht nicht im Raster.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Position aus der Anzahl der
/// impliziten Spalten VOR dieser einen berechnet wird und nicht
/// fortlaufend addiert: die impliziten Spalten stehen alle HINTER den
/// expliziten, und `ncol` ist ihre Grenze. Ohne Vorwaertszaehlung wuerde
/// die zweite implizite Spalte auf der ersten sitzen.
fn implizit_x(sp: usize, ncol: usize, breiten: &[f32; 32], gap: f32, implizit: f32) -> f32 {
    let mut x = 0.0f32;
    for b in breiten.iter().take(ncol) {
        x += *b + gap;
    }
    x + (sp.saturating_sub(ncol)) as f32 * (implizit + gap)
}

/// Der Index der Zelle (zeile, spalte) in der Belegungsmatrix.
///
/// L4 (02.10.2026 22:4x). GEMESSENER FEHLER MEINER ERSTEN FASSUNG,
/// gefunden von `l4_span_schoebt_den_cursor`: ich hatte EINEN Parameter
/// fuer den Zeilenabstand UND die Grenze benutzt (`zeile * grenze +
/// spalte`, begrenzt auf `grenze`). Bei einer Matrix aus `n*n` Feldern
/// und `n` Zeilen liegt die Zelle (1, 0) bei `n*n` — genau ausserhalb.
/// Die Spaltenzahl der Matrix war damit `1` statt `n`, und der Cursor
/// sah keine belegte Zelle.
///
/// GEMESSENE ENTSCHEIDUNG, warum die SPALTENZAHL ein eigener Parameter
/// ist und nicht aus `belegt.len()` folgt: sie ist nicht aus der Laenge
/// ableitbar (die Matrix ist quadratisch, `sqrt` waere eine
/// Wurzeloperation in einer Schleife, und sie stimmt nur bei
/// quadratischen). Die Grenze steht im `belegt`-Slice, und die loest die
/// Bindung auf.
fn feld(zeile: usize, spalte: usize, spalten: usize) -> Option<usize> {
    if spalten == 0 {
        return None;
    }
    Some(zeile * spalten + spalte)
}

/// Die Rechtecke der benannten Bereiche (§8.1, §8.4).
///
/// L4 (03.10.2026 02:11x). §8.4: „If the placement contains a named grid
/// area, ... the item's grid area is the named area" — also braucht
/// §8.4 das RECHTECK, nicht den Namen.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Rechteck hier gerechnet wird und
/// nicht in der Kaskade: das Rechteck ist eine Funktion aus der
/// Vorlage, und die kennt erst die Platzierung (die Spaltenzahl hängt
/// an der breitesten Zeile, §8.1).
///
/// GEMESSENE ENTSCHEIDUNG, warum die REIHENFOLGE zaehlt: §8.4 sagt, ein
/// Gebiet ist „the smallest rectangle that contains all the cells with
/// that name". Also braucht man nicht nur die erste Stelle, sondern das
/// kleinste umschliessende Rechteck — ein Name darf in einer Zeile
/// mehrmals stehen („`a a`") und dann ist es breiter.
fn gebiets_rechtecke(gebiete: &[Vec<crate::typen::Gebiet>]) -> Vec<gebiet_rechteck> {
    let mut raus: Vec<gebiet_rechteck> = Vec::new();
    // GEMESSENER EIGENER FEHLER (02:11x), gefunden vom Compiler und
    // benannt: ich hatte `zeile` als Name fuer die Zeilen-LISTE und fuer
    // den Zeilen-INDEX benutzt — zwei Bedeutungen in einer Schleife.
    for (z_index, z_names) in gebiete.iter().enumerate() {
        for (sp, g) in z_names.iter().enumerate() {
            if g.ist_leer() {
                // §8.1: ein leeres Gebiet belegt keine Spur.
                continue;
            }
            match raus.iter_mut().find(|t| t.name == *g) {
                Some(t) => {
                    // §8.4: „the smallest rectangle that contains all the
                    // cells with that name" — also beide Grenzen erweitern.
                    t.breite = t.breite.max(sp + 1 - t.spalte);
                    t.hoehe = t.hoehe.max(z_index + 1 - t.zeile);
                }
                None => raus.push(gebiet_rechteck {
                    name: *g,
                    zeile: z_index,
                    spalte: sp,
                    hoehe: 1,
                    breite: 1,
                }),
            }
        }
    }
    raus
}

/// Das Rechteck eines benannten Gebiets (§8.4).
///
/// L4 (03.10.2026 02:14x). GEMESSENE ENTSCHEIDUNG, warum eine STRUKTUR und
/// kein Tupel: ich hatte zuerst `(Gebiet, usize, usize, usize, usize)` und
/// `t.2` fuer die Spalte gehalten — es war die Zeilen-ENDgrenze (GEMESSEN:
/// `left: (1, 1)` statt `(1, 0)`). Vier gleiche Zahlen ohne Namen sind an der
/// Benutzungsstelle nicht lesbar; benannte Felder schon.
#[derive(Debug, Clone, Copy)]
pub struct gebiet_rechteck {
    /// Der Name des Gebiets.
    pub name: crate::typen::Gebiet,
    /// Die erste Zeile (0-basiert).
    pub zeile: usize,
    /// Die erste Spalte (0-basiert).
    pub spalte: usize,
    /// Die Anzahl der Zeilen (§8.4: „the smallest rectangle").
    pub hoehe: usize,
    /// Die Anzahl der Spalten.
    pub breite: usize,
}

/// Loest eine §8.3-Linie auf den 0-basierten Spurindex auf.
///
/// `anzahl` ist die Zahl der Spuren auf dieser Achse. L1basiert:
/// Linie 1 = Index 0. Negativ: von hinten, `-1` = die Linie HINTER der
/// letzten Spur (Index `anzahl`).
///
/// GEMESSENE ENTSCHEIDUNG, warum `-1` auf `anzahl` zeigt und nicht auf
/// `anzahl - 1`: §8.3 nennt `-1` „the last line", und bei `n` Spuren gibt
/// es `n+1` Linien (eine vor der ersten, eine nach der letzten). Die
/// letzte Linie ist die hinter der letzten Spur. GEMESSEN in
/// `l4_negative_linie_zeigt_auf_die_letzte`: ein 4-zeiliges Raster,
/// `-1` ergibt den Index 4.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine Linie, die AUSSERHALB liegt, zu
/// einem Index fuehrt statt zu `None`: §7.3/§7.5 erzeugen implizite
/// Spuren „as many as necessary", und §8.3 erlaubt ausdruecklich
/// Positionen jenseits der Vorlage. Nur ein Index, der auch negativ
/// waere, wird verworfen — das ist kein gueltiger Raster.
fn linie(l: i32, anzahl: usize) -> Option<usize> {
    if l == 0 {
        return None;
    }
    let idx = if l > 0 {
        l as isize - 1
    } else {
        anzahl as isize + 1 + l as isize
    };
    if idx < 0 { None } else { Some(idx as usize) }
}

/// Passt die Grid-Area (zeile, spalte, span, spann) komplett in freie
/// Zellen? (§8.5)
///
/// L4 (03.10.2026 00:6x). GEMESSENE ENTSCHEIDUNG, warum „passt die
/// GANZE Area" und nicht „ist die erste Zelle frei": GEMESSEN war
/// `[(0,0), (0,3), (1,2), (2,1)]` statt `[(0,0), (0,3), (1,0), (2,0)]` —
/// ein Kind am Zeilenende markierte mit seiner Spannweite Zellen der
/// naechsten Zeile. Die Suche muss also fragen, ob ALLE Zellen der
/// Area frei sind; das ist genau §8.5s „a grid area that is not occupied".
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zeilengrenze mit geprueft wird: ein
/// Item, das unten aus der Area herausragt, ist kein Platz in dieser
/// Grid-Area — es waere in einer impliziten Zeile, und §8.5 laesst den
/// Cursor weiterlaufen, statt es abzuschneiden.
fn passt(
    belegt: &[bool],
    zeile: usize,
    spalte: usize,
    ncol_eff: usize,
    max_zeilen: usize,
    span: usize,
    spann: usize,
) -> bool {
    if spalte + spann > ncol_eff || zeile + span > max_zeilen {
        return false;
    }
    for dz in 0..span {
        for ds in 0..spann {
            match feld(zeile + dz, spalte + ds, ncol_eff) {
                Some(p) if p < belegt.len() && belegt[p] => return false,
                _ => {}
            }
        }
    }
    true
}

/// §11.8 „STRETCH AUTO TRACKS": Spuren mit einer `auto`-Obergrenze
/// bekommen den restlichen Platz GLEICHMÄSSIG (CSS Grid 1 §11.8).
///
/// L4 (03.10.2026 05:5x). Wörtlich: „This step expands tracks that have
/// an auto max track sizing function by dividing any remaining positive,
/// definite free space equally amongst them."
///
/// GEMESSENE ENTSCHEIDUNG, warum „positiver" freier Platz eine Bedingung
/// ist und keine Redensart: §11.8 sagt „remaining POSITIVE, definite free
/// space". Es gibt keinen Platz, den man teilen könnte, wenn keiner da
/// ist. GEMESSEN (`tests/position.rs`,
/// `l4_auto_spalte_bleibt_ihrer_inhaltsbreite_wenn_der_platz_nicht_reicht`):
/// 180 px Kasten, zwei `auto`-Spuren mit 100 px Inhalt → **100 und 100**,
/// das Raster laeuft ueber. Mit `1fr` waeren es 90 und 90 gewesen — das
/// ist der Unterschied, den diese Funktion macht und den der `1fr`-Ersatz
/// nicht hatte.
///
/// GEMESSENE ENTSCHEIDUNG, warum die `auto`-Spalten hier UND die Spalten
/// mit Obergrenze bekommen: §11.5 laeuft zuerst und nimmt, was es
/// braucht, §11.8 danach — der Rest, der übrig bleibt, geht an die
/// `auto`-Spuren. Deshalb steht der Aufruf NACH `maximiere`.
///
/// GEMESSENE EINSCHRAENKUNG, und sie ist eine echte: §11.8 nennt auch den
/// Fall „if the free space is indefinite, but the grid container has a
/// definite min-width/height, use that size". `min-width` am Gitter
/// kennen wir nicht, und die Containerbreite ist hier immer die definite
/// Content-Box. Fuer den Rasterfluss ist die Zeilenachse (Containerhöhe)
/// nicht gebaut.
fn dehne_auto_spuren(breiten: &mut [f32; 32], spalten: &[SpaltenMass], container: f32, gap: f32) {
    let ncol = spalten.len().min(MAX_RASTER_SPALTEN);
    // GEMESSENE ENTSCHEIDUNG, warum die ABSTAENDE hier gerechnet werden
    // und nicht aus `breiten`: die Luecke steht nicht in `breiten`, sie
    // steht zwischen den Spuren. Ohne sie bekaeme der Rest zu viel und
    // das Raster waere um `n * gap` zu breit. `maximiere` rechnet es
    // genauso — dieselbe Form, dieselbe Grenze.
    let sichtbar = (0..ncol)
        .filter(|&i| !matches!(spalten[i], SpaltenMass::Auto { .. }))
        .count();
    let abstaende = gap * (sichtbar as f32 - 1.0).max(0.0);
    let belegt: f32 = breiten.iter().take(ncol).sum();
    let frei = (container - belegt - abstaende).max(0.0);
    let kandidaten: Vec<usize> = (0..ncol)
        .filter(|&i| matches!(spalten[i], SpaltenMass::Inhalt))
        .collect();
    if frei <= 0.0 || kandidaten.is_empty() {
        return;
    }
    let anteil = frei / kandidaten.len() as f32;
    for i in kandidaten {
        breiten[i] += anteil;
    }
}

/// Hebt Spuren bis zu ihrer Obergrenze an (CSS Grid 1 §11.5).
///
/// GEMESSENE ENTSCHEIDUNG, warum der freie Platz GLEICH verteilt wird
/// und nicht nach dem größten Zuwachs zuerst: §11.5 sagt „equally".
/// Die Kappung an der eigenen Grenze erzeugt die Asymmetrie von selbst —
/// die kleine Spur erreicht ihr Limit und nimmt danach nichts mehr.
///
/// GEMESSENE GRENZE, und sie ist eine echte: Der freie Platz wird nicht
/// wieder verteilt, wenn eine Spur ihre Grenze erreicht. §11.5 gibt
/// danach an `justify-content` weiter — das ist hier der
/// Blockfluss, und der kennt kein `justify-content` fuer Raster.
fn maximiere(breiten: &mut [f32; 32], spalten: &[SpaltenMass], container: f32, gap: f32) {
    let ncol = spalten.len().min(MAX_RASTER_SPALTEN);
    let belegt: f32 = breiten.iter().take(ncol).sum();
    let abstaende = gap * (ncol as f32 - 1.0).max(0.0);
    let mut frei = (container - belegt - abstaende).max(0.0);
    if frei <= 0.0 {
        return;
    }
    // Hoechstens `ncol` Runden: jede Runde hebt mindestens eine Spur auf
    // ihre Grenze, und es gibt hoechstens `ncol` Spuren. Die Schleife
    // ist damit ENDLICH — kein `while` mit unbekannter Abbruchbedingung.
    for _ in 0..ncol.max(1) {
        let kandidaten: Vec<usize> = (0..ncol)
            .filter(|&i| match spur_obergrenze(&spalten[i]) {
                Some(g) => breiten[i] < g,
                None => false,
            })
            .collect();
        if kandidaten.is_empty() {
            return;
        }
        let anteil = frei / kandidaten.len() as f32;
        let mut genutzt = 0.0f32;
        for &i in kandidaten.iter() {
            let grenze = spur_obergrenze(&spalten[i]).unwrap_or(breiten[i]);
            let nehmen = (grenze - breiten[i]).min(anteil).max(0.0);
            breiten[i] += nehmen;
            genutzt += nehmen;
        }
        frei -= genutzt;
        if frei <= 0.0 || genutzt <= 0.0 {
            return;
        }
    }
}
///
/// GEMESSENE ENTSCHEIDUNG, warum `frei` mit `+ gap` im Zaehler und im
/// Nenner steht: Nach `n` Spalten steht der `n`-te Abstand NICHT mehr
/// am Ende, sonst waere die Formel `n*w <= frei`. Mit `n-1` Abstaenden
/// lautet sie `n*w + (n-1)*gap <= frei`, also `n <= (frei + gap)/(w +
/// gap)`.
///
/// GEMESSENE ENTSCHEIDUNG, warum `Anteil` IMMER EINE ergibt: `1fr` hat
/// als Minimum `auto`, also 0, und damit waere die Zahl unendlich. CSS
/// behandelt eine `fr`-Spur in `auto-fill` als eine Spur — das ist die
/// Form, in der `auto-fill` fast nie benutzt wird, und GEMESSEN ist sie
/// genau die, die vorher „einmal angenommen" ergab.
///
/// GEMESSENE ENTSCHEIDUNG, warum mindestens EINE: `repeat(auto-fill,
/// 700px)` in 600 px ergibt nach CSS EINE Spalte, nicht null — das
/// Raster soll ja bestehen.
fn wiederholungen(spur: &SpaltenMass, frei: f32, gap: f32) -> usize {
    let w = spur_breite(spur);
    if w <= 0.0 {
        // GEMESSENE ENTSCHEIDUNG, warum hier 1 und nicht „unendlich":
        // `fr` hat als Minimum `auto` = 0, und `minmax(auto, 1fr)` auch.
        // CSS behandelt eine solche Spur in `auto-fill` als EINE Spur
        // (§7.2.3) — unendlich waere eine Liste bis `MAX_RASTER_SPALTEN`.
        return 1;
    }
    let n = ((frei + gap) / (w + gap)).floor();
    if n.is_finite() {
        (n as usize).clamp(1, MAX_RASTER_SPALTEN)
    } else {
        1
    }
}

/// Ersetzt jede `Auto`-Spalte durch so viele konkrete Spalten, wie in
/// den Container passen.
///
/// GEMESSENER BEFUND (`l4_raster.rs`, Release, 600 px Kasten, keine
/// Luecke) — vorher war JEDE dieser Zeilen einspaltig, weil
/// `parse.rs` die ganze Deklaration verworfen hat:
///
/// ```text
/// repeat(auto-fill, 100px)   1 Spalte  ->  6 Spalten
/// repeat(auto-fill, 250px)   1 Spalte  ->  2 Spalten
/// repeat(auto-fill, 700px)   1 Spalte  ->  1 Spalte
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum JEDE Gruppe gegen die volle Breite
/// zaehlt und nicht gegen den Rest: CSS Grid 1 §7.2.3 wiederholt jede
/// `auto-fill`/`auto-fit`-Gruppe fuer sich genommen: "repeat the track
/// list as many times as possible, without overflowing" — bezogen auf
/// die Spur der einen Gruppe, nicht auf die Nachbarn.
///
/// GEMESSENER FEHLER MEINER ERSTEN FASSUNG, und er war eine
/// Behauptung mit Zahlen dran: Ich hatte „sequenziell" gebaut (die
/// erste Gruppe nimmt, was sie braucht, die zweite zaehlt gegen den
/// Rest) und im Kommentar behauptet, zwei `repeat(auto-fill, 200px)`
/// in 600 px ergaeben 2 + 1 = 3 Spalten. GEMESSEN waren es **4**, und
/// die 3 waren von mir erfunden. Ein Browser macht nach §7.2.3
/// 3 + 3 = 6, mit Ueberlauf — und genau das ist die einfachere Regel.
/// Also: kein Rest, und die Behauptung ist weg.
///
/// GEMESSENE EINORDNUNG, warum das auf echten Seiten fast keine Rolle
/// spielt: Zwei `auto-fill`-Gruppen in EINER Vorlage sind selten. Die
/// GEMESSENEN Vorkommen auf dem Korpus (13) sind alle EINE Gruppe.
/// Der Fall ist also dokumentiert, nicht optimiert.
///
/// # L4 (10:5x): DIE ZWEITE RUECKGABE IST DAS `fit`-FLAG.
///
/// GEMESSENER GRUND: `auszaehlen` hat vorher N Kopien der SPUR
/// gemacht und das `fit` dabei weggeworfen. Damit konnte
/// `lege_raster_bei` eine kollabierte `auto-fit`-Spalte nicht von
/// einer `auto-fill`-Spalte unterscheiden — und §11.7.1 kollabiert
/// NUR die leeren `auto-fit`-Spuren.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein zweites `Vec<bool>` und kein
/// Feld in der Spur: ein Feld muesste fuer jede der drei anderen
/// Varianten eine Antwort tragen, die es dort nicht gibt. Ein
/// paralleler Vektor sagt nur das, was er wissen muss.
fn auszaehlen(c: f32, gap: f32, eingang: &[SpaltenMass]) -> (Vec<SpaltenMass>, Vec<bool>) {
    let n_max = MAX_RASTER_SPALTEN;
    let mut basis = 0usize;
    let mut fest = 0.0f32;
    for s in eingang.iter().take(n_max) {
        match s {
            SpaltenMass::Fest(v) => {
                fest += sauber_pos(*v);
                basis += 1;
            }
            SpaltenMass::Anteil(_) => basis += 1,
            SpaltenMass::Auto { .. } => {}
            // L4 (03.10.2026 05:4x): die `auto`-SPUR (§7.2) ist EINE
            // Grundspalte und keine Wiederholung — das ist genau der
            // Unterschied zu `Auto { spur, fit }` oben, wo die Zahl der
            // Spalten erst aus der Containerbreite kommt. Sie zaehlt in
            // `basis`, sonst waere ihr Platz in der Breitenrechnung
            // (`frei`) doppelt und einmal zu viel.
            SpaltenMass::Inhalt => basis += 1,
            // L4 (09:3x): die Prozent-Spur ist EINE Spalte und keine
            // Wiederholung. Ohne `basis += 1` waere ihr Platz in
            // `frei` doppelt und einmal zu viel (siehe `Inhalt`).
            SpaltenMass::Prozent(_) => basis += 1,
            // L4 (08:6x): `minmax` mit Anteil-Obergrenze ist eine
            // Grundspalte wie `Anteil`, mit Laengen-Obergrenze auch.
            SpaltenMass::MinMax { .. } => basis += 1,
        }
    }
    // Platz fuer die Wiederholungen: der Kasten minus die festen
    // Spalten minus die Abstaende der Grundspalten.
    let frei = c - fest - gap * (basis.saturating_sub(1) as f32);
    let mut raus: Vec<SpaltenMass> = Vec::with_capacity(n_max);
    let mut fit: Vec<bool> = Vec::with_capacity(n_max);
    for s in eingang.iter().take(n_max) {
        match s {
            SpaltenMass::Auto { spur, fit: f } => {
                let anzahl = wiederholungen(spur, frei, gap);
                for _ in 0..anzahl {
                    if raus.len() >= n_max {
                        break;
                    }
                    raus.push((**spur).clone());
                    fit.push(*f);
                }
            }
            // L4 (08:6x): `minmax` wird durchgereicht. `auto-fill` darf
            // auch DARAUS Spalten machen (`repeat(auto-fill,
            // minmax(200px, 1fr))`), und `auszaehlen` erzeugt sie dann
            // als N Kopien — das erledigt der Auto-Zweig.
            SpaltenMass::MinMax { .. } => {
                if raus.len() < n_max {
                    raus.push(s.clone());
                    fit.push(false);
                }
            }
            _ => {
                if raus.len() < n_max {
                    raus.push(s.clone());
                    fit.push(false);
                }
            }
        }
    }
    (raus, fit)
}

#[cfg(test)]
mod tests {
    // L4 (02.10.2026 08:5x): DIE ZAEHLUNG, abgesichert.
    //
    // GEMESSENE ENTSCHEIDUNG, warum hier Tests und nicht nur das
    // Messprogramm: `l4_raster.rs` zeigt die Zahl, aber nur fuer die
    // Blattform, die es selbst gebaut hat. Diese Tests setzen die Spur
    // direkt — damit ist die Zaehlung unabhaengig von Kaskade, Plan und
    // Leg, also genau an der Stelle, an der sie kaputt sein kann.
    use super::*;

    fn fest(v: f32) -> SpaltenMass {
        SpaltenMass::Fest(v)
    }
    fn fr(v: f32) -> SpaltenMass {
        SpaltenMass::Anteil(v)
    }

    fn n_spalten(c: f32, gap: f32, spur: &[SpaltenMass]) -> usize {
        auszaehlen(c, gap, spur).0.len()
    }

    #[test]
    fn auto_fill_zaehlt_sechs_von_hundert_in_sechshundert() {
        // floor(600/100) = 6
        assert_eq!(
            n_spalten(
                600.0,
                0.0,
                &[SpaltenMass::Auto {
                    spur: Box::new(fest(100.0)),
                    fit: false
                }]
            ),
            6
        );
    }

    #[test]
    fn auto_fill_zaehlt_mit_luecke_fuenf_statt_sechs() {
        // floor((600+20)/(100+20)) = floor(620/120) = 5
        //
        // GEMESSENE ENTSCHEIDUNG, warum dieser Test der wichtigste
        // ist: ohne die Luecke waeren es 6 — dieselbe Zahl wie ohne
        // Luecke. Der Fall unterscheidet also „mit Luecke gerechnet"
        // von „Luecke vergessen und dann zur Rechnung addiert".
        assert_eq!(
            n_spalten(
                600.0,
                20.0,
                &[SpaltenMass::Auto {
                    spur: Box::new(fest(100.0)),
                    fit: false
                }]
            ),
            5
        );
    }

    #[test]
    fn auto_fill_zweihundertfuenfzig_ergibt_zwei() {
        // floor(600/250) = 2 — NICHT 3, obwohl 3 x 250 <= 600 fast
        // aufgeht. Das ist der Fall, an dem eine Rundung sichtbar wird.
        assert_eq!(
            n_spalten(
                600.0,
                0.0,
                &[SpaltenMass::Auto {
                    spur: Box::new(fest(250.0)),
                    fit: false
                }]
            ),
            2
        );
    }

    #[test]
    fn auto_fill_zu_breit_ergibt_immer_noch_eine() {
        // 700 px passen keinmal in 600 px. GEMESSEN an der echten
        // Blattform: 1 Spalte, keine Null. Das Raster soll bestehen.
        assert_eq!(
            n_spalten(
                600.0,
                0.0,
                &[SpaltenMass::Auto {
                    spur: Box::new(fest(700.0)),
                    fit: false
                }]
            ),
            1
        );
    }

    #[test]
    fn auto_fill_anteil_ergibt_eine() {
        // `1fr` hat Minimum `auto` = 0, damit waere die Zahl
        // unendlich. CSS behandelt eine fr-Spur in auto-fill als eine.
        assert_eq!(
            n_spalten(
                600.0,
                0.0,
                &[SpaltenMass::Auto {
                    spur: Box::new(fr(1.0)),
                    fit: false
                }]
            ),
            1
        );
    }

    #[test]
    fn auto_fit_zaehlt_wie_auto_fill() {
        // GEMESSENE EINSCHRAENNKUNG: beide zaehlen heute gleich. Der
        // Unterschied von `auto-fit` — das Zusammenschrumpfen der
        // LEEREN Spuren — ist nicht gebaut. Dieser Test haelt die
        // ZAHLUNG fest und sagt mit seinem Namen, was er NICHT behauptet.
        let a = [SpaltenMass::Auto {
            spur: Box::new(fest(100.0)),
            fit: false,
        }];
        let b = [SpaltenMass::Auto {
            spur: Box::new(fest(100.0)),
            fit: true,
        }];
        assert_eq!(n_spalten(600.0, 0.0, &a), n_spalten(600.0, 0.0, &b));
    }

    #[test]
    fn zwei_auto_fill_zaehlen_gegen_die_volle_breite() {
        // CSS Grid 1 §7.2.3 wiederholt JEDE Gruppe fuer sich genommen
        // gegen die Containerbreite. Zwei Gruppen zu 200 px in 600 px
        // sind darum 3 + 3 = 6 Spalten — mit Ueberlauf, den §7.2.3
        // so auch beschreibt.
        //
        // GEMESSENER HINWEIS: mein erster Versuch teilte den Platz
        // nacheinander (4 Spalten) und mein Test behauptete 3. Beides
        // war falsch; §7.2.3 sagt 6. Der Test stand jetzt lange genug
        // rot, um die Behauptung zu kosten — das ist der Grund, warum
        // er hier mit der Zahl aus der Norm steht und nicht mit der,
        // die ich zuerst fuer richtig hielt.
        let s = [
            SpaltenMass::Auto {
                spur: Box::new(fest(200.0)),
                fit: false,
            },
            SpaltenMass::Auto {
                spur: Box::new(fest(200.0)),
                fit: false,
            },
        ];
        assert_eq!(n_spalten(600.0, 0.0, &s), 6);
    }

    #[test]
    fn auto_fill_haelt_die_reihenfolge() {
        // GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge ein Test und
        // keine Randnotiz ist: 100px fest, dann auto-fill 100px. Wenn
        // `auszaehlen` die festen Spalten nach vorn zoehe, kaeme das
        // erste Kind in Spalte 1 und das zweite in Spalte 2 statt 3 —
        // das Faelle ist 1 px breiter als es sein darf.
        let s = [
            fest(100.0),
            SpaltenMass::Auto {
                spur: Box::new(fest(100.0)),
                fit: false,
            },
        ];
        let (r, _fit) = auszaehlen(600.0, 0.0, &s);
        assert_eq!(r.len(), 6);
        assert_eq!(r[0], fest(100.0));
        assert_eq!(r[1], fest(100.0));
    }

    #[test]
    fn auto_fill_wird_durch_die_grenze_gekappt() {
        // 1 px in 10 000 px: 10 000 Spalten. `MAX_RASTER_SPALTEN` ist
        // 32. GEMESSENE ENTSCHEIDUNG, warum die Grenze KAPPT und nicht
        // ablehnt: `repeat(auto-fill, 1px)` ist gueltiges CSS, und die
        // Deklaration ist nicht falsch — nur absurd. Ein Ablehnen
        // wuerde die Vorlage verlieren und einspaltig legen.
        let s = [SpaltenMass::Auto {
            spur: Box::new(fest(1.0)),
            fit: false,
        }];
        assert_eq!(n_spalten(10_000.0, 0.0, &s), MAX_RASTER_SPALTEN);
    }

    #[test]
    fn ohne_auto_bleibt_es_bei_einer_spalte() {
        // Die Kontrollgruppe: eine `fr`-Spalte ohne `auto` ist EIN
        // Spalte. Wenn `auszaehlen` auch das aendern wuerde, waere
        // `1fr 1fr 1fr` ein 600-faches Raster.
        assert_eq!(n_spalten(600.0, 0.0, &[fr(1.0)]), 1);
    }
}

#[cfg(test)]
mod tests_alte {
    use super::*;

    fn kind(hoch: f32) -> RasterKind {
        RasterKind {
            hoch,
            ..RasterKind::default()
        }
    }
    fn fest(v: f32) -> SpaltenMass {
        SpaltenMass::Fest(v)
    }
    fn fr(v: f32) -> SpaltenMass {
        SpaltenMass::Anteil(v)
    }

    #[test]
    fn leer() {
        // L4 (02.10.2026 19:5x): ohne `grid-template-columns` ist das
        // KEIN leeres Raster (§7.2.1 nennt eine implizite Spur in der
        // Breite des Containers). GEMESSEN war hier vorher `ncol == 0 ->
        // return`, und die Engine hatte einen eigenen Zweig dafuer (19:4x)
        // — zwei Orte, eine Regel, zwei Antworten. Siehe
        // `tests/position.rs::c15_raster_ohne_kinder_oder_spalten_ist_leer`.
        let implizit = lege_raster(800.0, &[], 0.0, &[kind(10.0)]);
        assert_eq!(
            (implizit.len(), implizit[0].breite),
            (1, 800.0),
            "eine implizite Spur in der vollen Breite (§7.2.1)"
        );
        assert!(lege_raster(800.0, &[fest(100.0)], 0.0, &[]).is_empty());
    }
    #[test]
    fn feste_spalten() {
        let s = lege_raster(
            800.0,
            &[fest(100.0), fest(200.0)],
            10.0,
            &[kind(30.0), kind(40.0), kind(10.0)],
        );
        assert_eq!(s.len(), 3);
        assert_eq!(
            (s[0].x, s[0].breite, s[0].zeile, s[0].spalte),
            (0.0, 100.0, 0, 0)
        );
        assert_eq!((s[1].x, s[1].breite), (110.0, 200.0));
        // Zeilenhoehe = Maximum (40), drittes Kind in Zeile 1.
        assert_eq!((s[0].hoehe, s[2].y, s[2].zeile), (40.0, 50.0, 1));
    }
    #[test]
    fn fr_anteile() {
        // 800 - 2*10 Luecke = 780; 1fr:2fr -> 260/520.
        let s = lege_raster(800.0, &[fr(1.0), fr(2.0), fr(1.0)], 10.0, &[kind(5.0); 3]);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].breite, 195.0);
        assert_eq!(s[1].breite, 390.0);
        assert_eq!(s[2].x, 195.0 + 10.0 + 390.0 + 10.0);
    }
    #[test]
    fn gemischt_fest_fr() {
        // 100 fest + Rest 690 an 1fr.
        let s = lege_raster(
            800.0,
            &[fest(100.0), fr(1.0)],
            10.0,
            &[kind(5.0), kind(5.0)],
        );
        assert_eq!((s[0].breite, s[1].breite, s[1].x), (100.0, 690.0, 110.0));
    }
    #[test]
    fn fr_ohne_rest_null() {
        // Alles fest verbaut: fr bekommt 0.
        let s = lege_raster(100.0, &[fest(100.0), fr(1.0)], 0.0, &[kind(5.0), kind(5.0)]);
        assert_eq!(s[1].breite, 0.0);
    }
    #[test]
    fn zeilen_umbruch() {
        // 5 Kinder in 2 Spalten = 3 Zeilen (2/2/1).
        let s = lege_raster(800.0, &[fr(1.0), fr(1.0)], 0.0, &[kind(10.0); 5]);
        assert_eq!(s.len(), 5);
        assert_eq!((s[4].zeile, s[4].spalte), (2, 0));
        assert_eq!(s[4].y, 20.0);
    }
    #[test]
    fn unsinn() {
        let s = lege_raster(
            f32::NAN,
            &[fr(f32::INFINITY), fest(-5.0)],
            -3.0,
            &[kind(f32::NAN)],
        );
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].breite, s[0].hoehe), (0.0, 0.0));
    }
    #[test]
    fn kinder_deckel() {
        let viele = vec![kind(1.0); 5000];
        let s = lege_raster(800.0, &[fr(1.0)], 0.0, &viele);
        assert_eq!(s.len(), super::super::MAX_RASTER_ZELLEN);
    }
}
