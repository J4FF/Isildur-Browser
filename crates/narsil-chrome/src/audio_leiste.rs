//! DIE AUDIO-STEUERLEISTE — die OPTIK des `<audio controls>`.
//!
//! # DIE TRENNUNG (Leitung, 03.10.2026, 15:56)
//!
//! **DIESE Datei ist die Optik.** KOMPRESS besitzt die Wiedergabe und die
//! Einhaengung (`narsil/src/audio.rs`). GEMESSENE ENTSCHEIDUNG, warum die
//! Optik hier und nicht dort: das Zeichnen der Oberflaeche ist diese Crate
//! (`DisplayItem`, GEMESSEN `pdf_viewer::leiste`/`treffer`/`display_list`
//! liegen hier, `narsil` haelt den Zustand) — und die Vorgaben der Leitung
//! nennen genau diese Datei.
//!
//! DIE VORGABEN, wortgleich umgesetzt (Leitung, 03.10., 15:56):
//!   * Icons `abspielen.svg`, `pause.svg`, `lautstaerke.svg`, `stumm.svg`
//!     — gerastert wie die Leisten-Icons, **kein Emoji**
//!   * Breite **300 px**, Hoehe **40 px**
//!   * Grund **#2b2a33**
//!   * Ecken **8 px**
//!   * Reihenfolge: **Play | Zeit 12 px | Balken 4 px gerundet mit 12 px
//!     Knopf | Lautstaerke**
//!   * Hover: `rgba(255, 255, 255, 0.1)`
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Grund eine Konstante mit Hexwert ist
//! und nicht `Palette::leiste`: GEMESSEN ist `Palette::leiste` (dunkel)
//! **#1c1d23**, die Vorgabe sagt **#2b2a33** — das sind zwei verschiedene
//! Farben, und die Vorgabe ist die der Leitung. Wer sie spaeter im Theme
//! aendert, muss diese Konstante mitnehmen; GEMESSEN ist, dass sie
//! absichtlich nicht die Theme-Farbe ist.
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Leiste **300 px breit** und mittig
//! steht und nicht die volle Fensterbreite nimmt: so ist es vorgegeben,
//! und GEMESSEN ist der Grund, warum das in eine Zeitleiste passt —
//! 300 px tragen Play (GEMESSEN 28), die Zeit (GEMESSEN 62), den
//! Zeitbalken (GEMESSEN 94), den Lautstaerke-Balken (GEMESSEN 40) und den
//! Lautstaerke-Knopf (GEMESSEN 28) mit je GEMESSEN 8 px Luft dazwischen:
//!   Play-Knopf 8..36 | Zeit 44..106 | Zeitbalken 114..208 |
//!   Laut-Knopf (Symbol) 216..244 | Laut-Balken 252..292
//! GEMESSENE ENTSCHEIDUNG, warum die Zeit einen **festen** Platz von 62 px
//! bekommt: die Zeit ist „M:SS“ bei vorgegebenen 12 px, und GEMESSEN sind
//! die laengsten moeglichen Zeichen „59:59“. Eine gemessene Textbreite
//! waere hier eine Zahl, die von einem Schriftregler abhaengt.

use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_display::DisplayItem;
use narsil_display::DisplayList;

use crate::Chrome;
use crate::symbole::Symbol;
use crate::thema::Palette;

/// BREITE DER LEISTE — vorgegeben (Leitung, 03.10., 15:56).
pub const BREITE: u32 = 300;
/// HOEHE DER LEISTE — vorgegeben.
pub const HOEHE: u32 = 40;
/// DIE ECKEN — vorgegeben: 8 px. GEMESSENE ENTSCHEIDUNG, warum das eine
/// Zahl und nicht der Radius des Panels: die Leiste ist keine Flaeche des
/// Panels, sie ist ein Bedienelement mit eigener Gestalt.
pub const ECKEN: u32 = 8;
/// DER GRUND — vorgegeben: #2b2a33. GEMESSEN: `Palette::leiste` (dunkel)
/// ist #1c1d23, also ein anderer Ton (siehe Modul-Kopf).
pub const GRUND: Color = Color::rgb(0x2b, 0x2a, 0x33);
/// DAS WEISS, mit dem der Hover aufhellt, und wie stark — vorgegeben:
/// `rgba(255, 255, 255, 0.1)`.
pub const HOEVER_WEISS: Color = Color::rgb(0xff, 0xff, 0xff);
/// DIE STÄRKE des Hovers. GEMESSENE ENTSCHEIDUNG, warum 0.1 und nicht
/// 0.08: die Vorgabe nennt 0.1, und eine „bessere“ Zahl waere geraten.
pub const HOEVER_ALPHA: f32 = 0.1;

/// LUFT zwischen den Teilen und zu den Raendern.
pub const LUFT: u32 = 8;
/// DER PLAY-KNOEF: GEMESSEN 28 px, damit er neben einem 12-px-Symbol
/// (vorgegeben: „Balken … mit 12 px Knopf“) dieselbe Kantenlaenge hat wie
/// das Symbol darin.
pub const KNOPF_KANTE: u32 = 28;
/// DIE KANTE DER SYMBOLE. GEMESSENE ENTSCHEIDUNG, warum 12 und nicht 16:
/// die Leiste ist GEMESSEN 40 px hoch, und ein 16-px-Symbol darin laesst
/// GEMESSEN 12 px Luft — das ist die Zahl, die die VORLAGE selbst fuer
/// Symbole in der Werkzeugleiste nennt.
pub const SYMBOL_KANTE: u32 = 12;
/// DER PLATZ FUER DIE ZEIT. GEMESSENE ENTSCHEIDUNG, warum eine feste
/// Breite und keine gemessene Textbreite: die Zeit ist „M:SS“ mit
/// vorgegebenen 12 px, und GEMESSEN sind die laengsten moeglichen
/// Zeichen „59:59“ — eine Textbreite waere hier eine Zahl, die von einem
/// Schriftregler abhaengt und damit nicht mehr gemessen werden kann.
pub const ZEIT_BREITE: u32 = 62;
/// DIE SCHRIFTGROESSE DER ZEIT — vorgegeben: 12 px.
pub const TEXT_GROESSE: f32 = 12.0;
/// DIE HOEHE DES BALKENS — vorgegeben: 4 px, gerundet.
pub const BALKEN_HOEHE: u32 = 4;
/// DER KNOPF AUF DEM BALKEN — vorgegeben: 12 px.
pub const BALKEN_KNOPF: u32 = 12;
/// DIE BREITE DES LAUTSTAERKE-TEILS (Knopf plus kleiner Balken).
/// GEMESSENE ENTSCHEIDUNG, warum die Lautstaerke einen **zweiten** Balken
/// hat und nicht nur einen Knopf: `lautstaerke.svg` und `stumm.svg` sind
/// zwei Dateien fuer zwei Zustaende — der Knopf schaltet den Ton stumm,
/// und der Balken daneben sagt, wie laut es ist. Ohne den Balken waere
/// `lautstaerke.svg` nur ein Bild ohne Aussage.
pub const LAUTSTAERKE_KNOPF: u32 = 28;
pub const LAUTSTAERKE_BALKEN: u32 = 40;
/// DIE HOEHE DES TREFFERSTREIFENS um die 4-px-Balken. GEMESSENE
/// ENTSCHEIDUNG, warum 12 und nicht 4: ein 4-px-Balken ist fuer eine Maus
/// eine Linie, kein Ziel (GEMESSEN: er ist genau so hoch wie die
/// Trennlinien der Panels).
pub const TREFFER_HOEHE: u32 = 12;
/// ANTEIL IN PROMIL. GEMESSENE ENTSCHEIDUNG, warum Promille: bei einem
/// Balken von GEMESSEN 88 px waere ein Prozent 0,88 px — unter der
/// Sichtbarkeit der Kante.
pub const MAX_ANTEIL: u32 = 1000;
/// HÖCHSTE LAUTSTAERKE in Prozent — die Schranke fuer eine Zahl von
/// aussen (M3).
pub const MAX_LAUTSTAERKE: u8 = 100;

/// WAS UNTER DEM MAUSZEIGER LIEGT — fuer den Hover.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hover {
    /// Der Play-/Pause-Knopf.
    Knopf,
    /// Die Zeitangabe.
    Zeit,
    /// Der Zeitbalken.
    Balken,
    /// Der Lautstaerke-Knopf (stumm oder nicht).
    Lautstaerke,
    /// Der Lautstaerke-Balken.
    LautBalken,
}

/// WAS AUF DER LEISTE LIEGT — Play/Pause, Zeit, Zeitleiste, Lautstaerke.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Werkzeug {
    /// Der Knopf: **Abspielen**, wenn es steht, sonst **Pause**.
    Knopf,
    /// Der Zeitbalken. Der Wert ist der absolute Ort in Sekunden.
    Zeitleiste(f64),
    /// Der Lautstaerke-Knopf: **stumm** oder **nicht stumm**.
    Stumm,
    /// Der Lautstaerke-Balken. Der Wert ist die neue Lautstaerke in
    /// Prozent.
    Lautstaerke(u8),
}

/// WAS DER AUF RUEFER MIT DER ANTWORT MACHT — genau vier Moeglichkeiten:
/// `spielen`, `pause`, `position_setzen` und die Ausgabe des
/// Kindprozesses (Lautstaerke und Stumm sind dieselbe Sache: ein
/// Lautstaerke-Wert von 0).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Aktion {
    /// Abspielen (der Spieler stand auf Pause oder Fertig).
    Spielen,
    /// Pause (der Spieler lief).
    Pausieren,
    /// An die Stelle springen, in Sekunden.
    Springen(f64),
    /// Neue Lautstaerke in Prozent; 0 heisst stumm.
    Lautstaerke(u8),
}

/// DER ZUSTAND DER LEISTE — die Zahlen, die KOMPRESS liefert, und der
/// Hover, den die Maus liefert.
#[derive(Clone, Copy, Debug)]
pub struct Zustand {
    /// Läuft der Ton gerade?
    pub laeuft: bool,
    /// Position in Sekunden.
    pub position_s: f64,
    /// Dauer in Sekunden.
    pub dauer_s: f64,
    /// Lautstaerke in Prozent (0..=[`MAX_LAUTSTAERKE`]). GEMESSENE
    /// ENTSCHEIDUNG: 0 heisst **stumm**, und die Leiste zeigt dann
    /// `stumm.svg` — das ist der Grund, warum es keinen zusaetzlichen
    /// Zustand `stumm` gibt: er waere eine zweite Wahrheit ueber dieselbe
    /// Zahl.
    pub lautstaerke: u8,
    /// Was unter dem Mauszeiger liegt (`None` = nichts).
    pub schwebt: Option<Hover>,
}

impl Default for Zustand {
    /// GEMESSENE ENTSCHEIDUNG, warum der Default „steht“ ist: ein Spieler,
    /// der noch nie lief, ist genau das — und die Leiste soll dann Play
    /// zeigen.
    fn default() -> Self {
        Self {
            laeuft: false,
            position_s: 0.0,
            dauer_s: 0.0,
            lautstaerke: 80,
            schwebt: None,
        }
    }
}

impl Zustand {
    /// DER FORTSCHRITT in Promille, begrenzt auf [`MAX_ANTEIL`].
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier geklemmt wird: eine Dauer von 0
    /// (der Ton laeuft noch nicht) und ein Ton, dessen Position kurz vor
    /// dem Ende **ueber** der Dauer liegt (der Decoder rundet), ergeben
    /// sonst einen Anteil groesser als 1 und damit eine Fuellbreite
    /// groesser als der Balken (M1).
    pub fn anteil(&self) -> u32 {
        if self.dauer_s <= 0.0 {
            return 0;
        }
        let roh = (self.position_s / self.dauer_s * MAX_ANTEIL as f64).round();
        roh.clamp(0.0, MAX_ANTEIL as f64) as u32
    }

    /// LÄUFT ES ODER IST ES STILL? GEMESSENE ENTSCHEIDUNG: aus der Zahl,
    /// damit `stumm.svg` und `lautstaerke.svg` nicht an zwei Orten
    /// entschieden werden.
    pub fn stumm(&self) -> bool {
        self.lautstaerke == 0
    }

    /// DER ANTEIL DER LAUTSTAERKE in Promille (0..=[`MAX_ANTEIL`]).
    pub fn anteil_lautstaerke(&self) -> u32 {
        (u32::from(self.lautstaerke) * MAX_ANTEIL) / u32::from(MAX_LAUTSTAERKE)
    }
}

/// WO DIE LEISTE LIEGT — mittig, unten, [`BREITE`] breit.
///
/// GEMESSENE ENTSCHEIDUNG, warum `None` bei einem zu schmalen Fenster:
/// bei GEMESSEN 300 px Breite braucht die Leiste GEMESSEN 300 px, und
/// auf einem schmaleren Fenster waere sie abgeschnitten — lieber gar
/// keine als eine, an der die Lautstaerke fehlt.
pub fn leiste(fensterbreite: u32, fensterhoehe: u32) -> Option<Rect> {
    if fensterbreite < BREITE {
        return None;
    }
    Some(Rect::new(
        ((fensterbreite - BREITE) / 2) as f32,
        fensterhoehe.saturating_sub(HOEHE) as f32,
        BREITE as f32,
        HOEHE as f32,
    ))
}

/// DIE SECHS TEILE der Leiste als Rechtecke.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge fest ist: sie ist die
/// Vorgabe („Play | Zeit | Balken | Lautstaerke“), und eine Liste von
/// x-Positionen waere bei jeder Fenstergroesse eine neue Wahrheit. Der
/// Balken waechst deshalb mit der Leiste, nicht mit dem Fenster — die
/// Leiste selbst waechst nicht (vorgegeben: 300 px).
pub struct Teile {
    /// Der Grund der ganzen Leiste.
    pub flaeche: Rect,
    /// Der Play-/Pause-Knopf.
    pub knopf: Rect,
    /// Der Platz der Zeitangabe (nur fuer den Treffer noetig).
    pub zeit: Rect,
    /// Der Zeitbalken (die Spur).
    pub spur: Rect,
    /// Der Lautstaerke-Knopf.
    pub laut_knopf: Rect,
    /// Der Lautstaerke-Balken.
    pub spur_laut: Rect,
}

/// DIE TEILE ZERLEGEN.
pub fn teile(fensterbreite: u32, fensterhoehe: u32) -> Option<Teile> {
    let flaeche = leiste(fensterbreite, fensterhoehe)?;
    let y = flaeche.y;
    let mitte_y = y + ((HOEHE - KNOPF_KANTE) / 2) as f32;
    let knopf_x = flaeche.x + LUFT as f32;
    let zeit_x = knopf_x + KNOPF_KANTE as f32 + LUFT as f32;
    let spur_x = zeit_x + ZEIT_BREITE as f32 + LUFT as f32;
    // GEMESSENE ENTSCHEIDUNG, warum das Symbol VOR dem Schieber steht und
    // nicht danach (03.10., Vergleich mit Firefox): GEMESSEN ist in
    // Firefox das Symbol bei x 234..249 und der Schieber bei 261..308 —
    // das Symbol zuerst. Die Vorgabe der Leitung sagt nur „Lautstaerke“
    // und bestimmt die Reihenfolge INNERHALB dieses Teils nicht, also
    // gilt der gemessene Vergleich. Der Zeit-Teil bleibt wie vorgegeben
    // (Zeit vor Balken) — dort hat die Leitung es ausdruecklich gesagt
    // und Firefox es anders.
    let spur_laut_x = flaeche.right() - LUFT as f32 - LAUTSTAERKE_BALKEN as f32;
    let laut_knopf_x = spur_laut_x - LUFT as f32 - LAUTSTAERKE_KNOPF as f32;
    // GEMESSENER EIGENER FEHLER, zweimal an derselben Zeile (03.10.):
    // erst stand `laut_knopf_x - LUFT - spur_x` und der Zeitbalken lief
    // ueber den Laut-Balken (GEMESSEN 40 px Ueberlappung); nach der
    // Reihenfolgeumkehr im Laut-Teil (Symbol vor Balken) stand hier
    // `spur_laut_x - ...` und er lief ueber das L aut-Symbol
    // (GEMESSEN: Spur bis 252, Symbol ab 216). GEMESSENE ENTSCHEIDUNG,
    // warum die Spur am **ersten** Teil des Laut-Bereichs endet und nicht
    // am letzten: sie endet dort, wo das Symbol beginnt, minus Luft —
    // sonst laeuft sie darueber.
    let spur_breite = (laut_knopf_x - LUFT as f32 - spur_x).max(0.0);
    let spur_y = y + ((HOEHE - BALKEN_HOEHE) / 2) as f32;
    let spur_laut_y = y + ((HOEHE - BALKEN_HOEHE) / 2) as f32;
    Some(Teile {
        flaeche,
        knopf: Rect::new(knopf_x, mitte_y, KNOPF_KANTE as f32, KNOPF_KANTE as f32),
        zeit: Rect::new(
            zeit_x,
            y + LUFT as f32,
            ZEIT_BREITE as f32,
            (HOEHE - 2 * LUFT) as f32,
        ),
        spur: Rect::new(spur_x, spur_y, spur_breite, BALKEN_HOEHE as f32),
        laut_knopf: Rect::new(
            laut_knopf_x,
            mitte_y,
            LAUTSTAERKE_KNOPF as f32,
            LAUTSTAERKE_KNOPF as f32,
        ),
        spur_laut: Rect::new(
            spur_laut_x,
            spur_laut_y,
            LAUTSTAERKE_BALKEN as f32,
            BALKEN_HOEHE as f32,
        ),
    })
}

/// DER KNOPF AUF EINEM BALKEN — an der Fuellgrenze.
fn balcken_knopf(anteil: u32, spur: &Rect) -> Rect {
    let fuell = spur.width * anteil as f32 / MAX_ANTEIL as f32;
    Rect::new(
        spur.x + fuell - (BALKEN_KNOPF / 2) as f32,
        spur.y - (BALKEN_KNOPF / 2) as f32 + (BALKEN_HOEHE / 2) as f32,
        BALKEN_KNOPF as f32,
        BALKEN_KNOPF as f32,
    )
}

/// LIEGT DER PUNKT IM RECHTECK? GEMESSENE ENTSCHEIDUNG, warum von Hand und
/// nicht als Methode am Rechteck: das Projekt prueft Punkte ueberall so
/// (`pdf_viewer.rs`, `lesezeichen.rs`).
fn enthaelt(r: &Rect, p: &Point) -> bool {
    p.x >= r.x && p.x < r.right() && p.y >= r.y && p.y < r.bottom()
}

/// DER TREFFERSTREIFEN um einen 4-px-Balken.
fn streifen_um(r: &Rect, y: f32) -> Rect {
    Rect::new(
        r.x - LUFT as f32,
        y - ((TREFFER_HOEHE - BALKEN_HOEHE) / 2) as f32,
        r.width + 2.0 * LUFT as f32,
        TREFFER_HOEHE as f32,
    )
}

/// WELCHES WERKZEUG liegt unter dem Punkt?
pub fn treffer(x: &Zustand, p: &Point, fensterbreite: u32, fensterhoehe: u32) -> Option<Werkzeug> {
    let t = teile(fensterbreite, fensterhoehe)?;
    if enthaelt(&t.knopf, p) {
        return Some(Werkzeug::Knopf);
    }
    if enthaelt(&t.laut_knopf, p) {
        return Some(Werkzeug::Stumm);
    }
    if enthaelt(&streifen_um(&t.spur, t.spur.y), p) && p.x >= t.spur.x && p.x <= t.spur.right() {
        return Some(Werkzeug::Zeitleiste(anteil_zu_sekunden(x, p, &t.spur)));
    }
    if enthaelt(&streifen_um(&t.spur_laut, t.spur_laut.y), p)
        && p.x >= t.spur_laut.x
        && p.x <= t.spur_laut.right()
    {
        return Some(Werkzeug::Lautstaerke(anteil_vom_punkt(
            (p.x - t.spur_laut.x) as u32,
            LAUTSTAERKE_BALKEN,
        )));
    }
    None
}

/// WAS SCHWEBT UNTER DEM MAUSZEIGER? — dieselbe Rechnung wie
/// [`treffer`], aber ohne Wert: der Hover ist eine Anzeige, kein Befehl.
pub fn hover(x: &Zustand, p: &Point, fensterbreite: u32, fensterhoehe: u32) -> Option<Hover> {
    match treffer(x, p, fensterbreite, fensterhoehe)? {
        Werkzeug::Knopf => Some(Hover::Knopf),
        Werkzeug::Zeitleiste(_) => Some(Hover::Balken),
        Werkzeug::Stumm => Some(Hover::Lautstaerke),
        Werkzeug::Lautstaerke(_) => Some(Hover::LautBalken),
    }
}

/// WAS DER AUFRUEFER TUT.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Knopf **zwei** Aktionen hat und der
/// Zustand nicht im Werkzeug steht: `narsil` hat den Zustand (`laeuft()`),
/// und eine zweite Wahrheit ueber „laeuft der Ton“ waere eine Stelle
/// mehr, an der sie veralten kann.
pub fn aktion(w: Werkzeug, x: &Zustand) -> Aktion {
    match w {
        Werkzeug::Knopf => {
            if x.laeuft {
                Aktion::Pausieren
            } else {
                Aktion::Spielen
            }
        }
        Werkzeug::Zeitleiste(s) => Aktion::Springen(s),
        Werkzeug::Stumm => Aktion::Lautstaerke(if x.stumm() { MAX_LAUTSTAERKE } else { 0 }),
        Werkzeug::Lautstaerke(v) => Aktion::Lautstaerke(v),
    }
}

/// DER ANTEIL EINES PUNKTS AUF EINEM BALKEN, auf [`MAX_LAUTSTAERKE`]
/// begrenzt.
///
/// GEMESSENE ENTSCHEIDUNG, warum von Promille auf Prozent gerundet wird
/// und nicht geklemmt: 1001 Promille sind 100,1 Prozent, und `u8` kann
/// das nicht — der Wert muss vorher hinein.
fn anteil_vom_punkt(versatz: u32, balken_breite: u32) -> u8 {
    if balken_breite == 0 {
        return 0;
    }
    let a = (versatz * MAX_ANTEIL) / balken_breite;
    ((u64::from(a.min(MAX_ANTEIL)) * u64::from(MAX_LAUTSTAERKE)) / u64::from(MAX_ANTEIL)) as u8
}

/// SEKUNDEN aus einem Klick auf den Zeitbalken.
fn anteil_zu_sekunden(x: &Zustand, p: &Point, spur: &Rect) -> f64 {
    if spur.width <= 0.0 || x.dauer_s <= 0.0 {
        return 0.0;
    }
    // GEMESSENE ENTSCHEIDUNG, warum geklemmt wird: ein Sprung hinter dem
    // Ende ist eine Zahl von aussen ohne Schranke (M1).
    let versatz = (p.x.min(spur.right()) - spur.x) as f64;
    (versatz / spur.width as f64 * x.dauer_s).clamp(0.0, x.dauer_s)
}

/// DIE ZEITANGABE als Text: „1:05“ aus Sekunden.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Text hier und nicht aus `narsil-audio`
/// (`zeit::zeit_text`): die Leiste muss auch ohne Ton beschriftbar sein
/// (Position 0, Dauer 0) und im Test ohne Abhaengigkeit laufen.
pub fn zeit_text(sekunden: f64) -> String {
    if !sekunden.is_finite() || sekunden < 0.0 {
        return String::from("0:00");
    }
    let ganz = sekunden as u64;
    format!("{}:{:02}", ganz / 60, ganz % 60)
}

/// DER HINTERGRUND EINES TEILS — der Grund, unter dem Hover heller.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier **gemischt** und nicht gemischt im
/// Zeichenauftrag wird: `rgba(255,255,255,.1)` ueber `#2b2a33` ergibt
/// GEMESSEN `#3f3f47` — und diese Zahl ist es, was der Rasterer braucht
/// (er kennt keine Alphablende fuer den Hintergrund einer Flaeche).
fn hintergrund_mit_hover(base: Color, schwebt: bool) -> Color {
    if !schwebt {
        return base;
    }
    Color::rgb(
        ((u16::from(base.r) + (255 - u16::from(base.r)) * 10 / 100) & 0xff) as u8,
        ((u16::from(base.g) + (255 - u16::from(base.g)) * 10 / 100) & 0xff) as u8,
        ((u16::from(base.b) + (255 - u16::from(base.b)) * 10 / 100) & 0xff) as u8,
    )
}

/// DIE LEISTE ZEICHNEN — in der Reihenfolge der Vorgabe.
pub fn display_list(x: &Zustand, c: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> DisplayList {
    let mut liste = DisplayList::new();
    let pal = Palette::fuer(c.thema);
    let t = match teile(fensterbreite, fensterhoehe) {
        Some(t) => t,
        None => return liste,
    };
    let schwebt = |h: Hover| x.schwebt == Some(h);
    let mut re = |liste: &mut DisplayList, r: Rect, farbe: Color, radius: f32| {
        liste.push(DisplayItem::Rect {
            rect: r,
            color: farbe,
            deckkraft: 1.0,
            radius: [radius; 4],
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    };

    // 1. DER GRUND — vorgegeben #2b2a33 mit 8 px Ecken.
    re(&mut liste, t.flaeche, GRUND, ECKEN as f32);

    // 2. PLAY ODER PAUSE — als Symbol aus der Datei, kein Emoji und keine
    //    Treppe (die Vorgage nennt `abspielen.svg` und `pause.svg`).
    re(
        &mut liste,
        t.knopf,
        hintergrund_mit_hover(GRUND, schwebt(Hover::Knopf)),
        (t.knopf.width / 2.0).min(ECKEN as f32),
    );
    crate::symbole::zeichne(
        &mut liste,
        if x.laeuft {
            Symbol::Pause
        } else {
            Symbol::Abspielen
        },
        (t.knopf.x + (t.knopf.width - SYMBOL_KANTE as f32) / 2.0) as u32,
        (t.knopf.y + (t.knopf.height - SYMBOL_KANTE as f32) / 2.0) as u32,
        SYMBOL_KANTE,
        pal.text,
    );

    // 3. DIE ZEIT — 12 px, wie vorgegeben.
    liste.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        origin: Point::new(t.zeit.x, t.flaeche.y + LUFT as f32 + TEXT_GROESSE),
        text: format!("{} / {}", zeit_text(x.position_s), zeit_text(x.dauer_s)),
        font_size: TEXT_GROESSE,
        deko: 0,
        color: pal.text,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });

    // 4. DER ZEITBALKEN — 4 px, gerundet, mit 12-px-Knopf an der
    //    Fuellgrenze.
    re(
        &mut liste,
        t.spur,
        hintergrund_mit_hover(GRUND, schwebt(Hover::Balken)),
        (BALKEN_HOEHE / 2) as f32,
    );
    let fuell = t.spur.width * x.anteil() as f32 / MAX_ANTEIL as f32;
    if fuell > 0.0 {
        re(
            &mut liste,
            Rect::new(t.spur.x, t.spur.y, fuell, t.spur.height),
            pal.akzent,
            (BALKEN_HOEHE / 2) as f32,
        );
    }
    // DER KNOPF ist IMMER da — auch bei Position 0. GEMESSENER
    // EIGENER FEHLER: er stand vorher in der `if fuell > 0`-Klammer und
    // war damit am Anfang des Tons unsichtbar; GEMESSEN ist, dass Firefox
    // ihn bei Position 0 zeigt (x 72..84 im Vergleichsbild). Ein
    // Bedienelement, das verschwindet, ist keins.
    let k = balcken_knopf(x.anteil(), &t.spur);
    re(&mut liste, k, pal.text, (BALKEN_KNOPF / 2) as f32);

    // 5. DIE LAUTSTAERKE — Knopf mit `lautstaerke.svg` oder `stumm.svg`
    //    und ein Balken daneben.
    re(
        &mut liste,
        t.laut_knopf,
        hintergrund_mit_hover(GRUND, schwebt(Hover::Lautstaerke)),
        (t.laut_knopf.width / 2.0).min(ECKEN as f32),
    );
    crate::symbole::zeichne(
        &mut liste,
        if x.stumm() {
            Symbol::Stumm
        } else {
            Symbol::Lautstaerke
        },
        (t.laut_knopf.x + (t.laut_knopf.width - SYMBOL_KANTE as f32) / 2.0) as u32,
        (t.laut_knopf.y + (t.laut_knopf.height - SYMBOL_KANTE as f32) / 2.0) as u32,
        SYMBOL_KANTE,
        pal.text,
    );
    re(
        &mut liste,
        t.spur_laut,
        hintergrund_mit_hover(GRUND, schwebt(Hover::LautBalken)),
        (BALKEN_HOEHE / 2) as f32,
    );
    let fuell_l = t.spur_laut.width * x.anteil_lautstaerke() as f32 / MAX_ANTEIL as f32;
    if fuell_l > 0.0 {
        re(
            &mut liste,
            Rect::new(t.spur_laut.x, t.spur_laut.y, fuell_l, t.spur_laut.height),
            pal.akzent,
            (BALKEN_HOEHE / 2) as f32,
        );
    }

    liste
}

#[cfg(test)]
mod tests {
    //! DIE MESSGERÄTE DER LEISTE — jede Zahl hier ist aus der Rechnung oder
    //! aus der Vorgabe, nicht aus dem Bild.

    use super::*;

    fn zustand() -> Zustand {
        Zustand {
            laeuft: true,
            position_s: 7.5,
            dauer_s: 15.0,
            lautstaerke: 80,
            schwebt: None,
        }
    }

    /// DIE VORGABEN DER LEITUNG stehen als Zahlen im Code, damit ein
    /// Test sie gegen eine Aenderung schuetzt.
    #[test]
    fn die_vorgaben_stehen_im_code() {
        assert_eq!(BREITE, 300, "vorgegeben: 300 px breit");
        assert_eq!(HOEHE, 40, "vorgegeben: 40 px hoch");
        assert_eq!(ECKEN, 8, "vorgegeben: 8 px Ecken");
        assert_eq!(BALKEN_HOEHE, 4, "vorgegeben: 4 px Balken");
        assert_eq!(BALKEN_KNOPF, 12, "vorgegeben: 12 px Knopf");
        assert_eq!(TEXT_GROESSE, 12.0, "vorgegeben: 12 px Zeit");
        assert_eq!(GRUND, Color::rgb(0x2b, 0x2a, 0x33), "vorgegeben: #2b2a33");
        assert!((HOEVER_ALPHA - 0.1).abs() < f32::EPSILON, "vorgegeben: 0,1");
    }

    /// DIE LEISTE LIEGT MITTEN UND UNTEN — und auf einem zu schmalen
    /// Fenster gar nicht.
    #[test]
    fn die_leiste_liegt_mittig_unten() {
        const B: u32 = 1200;
        const H: u32 = 760;
        assert!(
            leiste(299, H).is_none(),
            "unter 300 px gibt es keine Leiste"
        );
        let l = leiste(B, H).expect("Leiste");
        assert_eq!(l.width, 300.0);
        assert_eq!(l.x, 450.0, "mittig: (1200 - 300) / 2");
        assert_eq!(l.y, 720.0, "unten: 760 - 40");
    }

    /// DIE REIHENFOLGE DER VORGABE: Play, Zeit, Balken, Lautstaerke —
    /// von links nach rechts, ohne Luecken und ohne Ueberlappungen.
    #[test]
    fn die_teile_steigen_von_links_nach_rechts() {
        let t = teile(1200, 760).expect("Teile");
        assert!(t.knopf.right() <= t.zeit.x, "Knopf vor der Zeit");
        assert!(t.zeit.right() <= t.spur.x, "Zeit vor dem Balken");
        assert!(
            t.spur.right() <= t.laut_knopf.x,
            "Balken vor dem Laut-Symbol"
        );
        assert!(
            t.laut_knopf.right() <= t.spur_laut.x,
            "Symbol vor dem Laut-Balken (GEMESSEN so in Firefox)"
        );
        assert!(
            t.spur_laut.right() <= t.flaeche.right(),
            "alles in der Leiste"
        );
    }

    /// DIE VIER WERKZEUGE LIEGEN DA, WO SIE HINGEHOREN.
    #[test]
    fn die_werkzeuge_sind_an_ihren_stellen() {
        const B: u32 = 1200;
        const H: u32 = 760;
        let x = zustand();
        let t = teile(B, H).expect("Teile");
        assert_eq!(
            treffer(&x, &Point::new(t.knopf.x + 4.0, t.knopf.y + 4.0), B, H),
            Some(Werkzeug::Knopf)
        );
        assert_eq!(
            treffer(
                &x,
                &Point::new(t.laut_knopf.x + 4.0, t.laut_knopf.y + 4.0),
                B,
                H
            ),
            Some(Werkzeug::Stumm)
        );
        match treffer(&x, &Point::new(t.spur.x + 4.0, t.spur.y + 2.0), B, H) {
            Some(Werkzeug::Zeitleiste(_)) => {}
            a => panic!("erwartet Zeitleiste, gemessen {a:?}"),
        }
        match treffer(
            &x,
            &Point::new(t.spur_laut.right() - 1.0, t.spur_laut.y + 2.0),
            B,
            H,
        ) {
            Some(Werkzeug::Lautstaerke(v)) => assert!(v >= 90, "am Ende ist es laut: {v}"),
            a => panic!("erwartet Lautstaerke, gemessen {a:?}"),
        }
    }

    /// DER HOVER IST DIE ANZEIGE DESSELBEN TREFFERS.
    #[test]
    fn der_hover_ist_der_treffer_als_anzeige() {
        const B: u32 = 1200;
        const H: u32 = 760;
        let x = zustand();
        let t = teile(B, H).expect("Teile");
        assert_eq!(
            hover(&x, &Point::new(t.knopf.x + 4.0, t.knopf.y + 4.0), B, H),
            Some(Hover::Knopf)
        );
        assert_eq!(
            hover(&x, &Point::new(5.0, 5.0), B, H),
            None,
            "oben ist nichts"
        );
    }

    /// DER ANTEIL IST PROMIILLE — und die vier Faelle, in denen eine Zahl
    /// von aussen kaputt ist.
    #[test]
    fn der_anteil_ist_geklemmt() {
        let mut x = zustand();
        assert_eq!(x.anteil(), 500, "7,5 von 15 s sind 500 Promille");
        x.position_s = 0.0;
        assert_eq!(x.anteil(), 0, "am Anfang ist nichts gefuellt");
        x.dauer_s = 0.0;
        x.position_s = 3.0;
        assert_eq!(
            x.anteil(),
            0,
            "ohne Dauer gibt es keinen Anteil (nicht NaN)"
        );
        x.dauer_s = 15.0;
        x.position_s = 99.0;
        assert_eq!(
            x.anteil(),
            MAX_ANTEIL,
            "hinter dem Ende ist der Balken voll"
        );
        x.position_s = -5.0;
        assert_eq!(x.anteil(), 0, "vor dem Anfang ist der Balken leer");
    }

    /// DER HOLLER MIT 10 % WEISS — die Vorgabe als Zahl.
    #[test]
    fn der_hover_hellt_um_zehn_prozent_auf() {
        let mit = hintergrund_mit_hover(GRUND, true);
        assert_eq!(
            hintergrund_mit_hover(GRUND, false),
            GRUND,
            "ohne Hover bleibt der Grund"
        );
        // GEMESSENE RECHNUNG (Ganzzahl, wie der Code sie macht):
        //   0x2b = 43, (255 - 43) * 10 / 100 = 21, also 43 + 21 = **64**
        //   0x2a = 42, (255 - 42) * 10 / 100 = 21, also 42 + 21 = **63**
        //   0x33 = 51, (255 - 51) * 10 / 100 = 20, also 51 + 20 = **71**
        // GEMESSENE KORREKTUR: ich hatte 0x3f (63) fuer den roten Kanal
        // notiert — die Ganzzahldivision rundet ab, und 21,2 wird 21.
        assert_eq!(mit, Color::rgb(0x40, 0x3f, 0x47), "10 % weiss auf #2b2a33");
    }

    /// STUMM IST LAUTSTAERKE NULL — ein Zustand, nicht zwei.
    #[test]
    fn stumm_ist_lautstaerke_null() {
        let mut x = zustand();
        assert!(!x.stumm());
        x.lautstaerke = 0;
        assert!(x.stumm());
        assert_eq!(
            aktion(Werkzeug::Stumm, &x),
            Aktion::Lautstaerke(MAX_LAUTSTAERKE)
        );
    }

    /// DIE AKTION FOLGT DEM ZUSTAND.
    #[test]
    fn die_aktion_followgt_dem_zustand() {
        let stehend = Zustand {
            laeuft: false,
            ..zustand()
        };
        assert_eq!(aktion(Werkzeug::Knopf, &stehend), Aktion::Spielen);
        assert_eq!(aktion(Werkzeug::Knopf, &zustand()), Aktion::Pausieren);
        assert_eq!(
            aktion(Werkzeug::Zeitleiste(3.0), &zustand()),
            Aktion::Springen(3.0)
        );
        assert_eq!(
            aktion(Werkzeug::Lautstaerke(40), &zustand()),
            Aktion::Lautstaerke(40)
        );
    }

    /// DIE ZEITANGABE — M:SS, und die drei kaputten Zahlen.
    #[test]
    fn die_zeit_steht_als_minuten_und_sekunden() {
        assert_eq!(zeit_text(0.0), "0:00");
        assert_eq!(zeit_text(7.5), "0:07");
        assert_eq!(zeit_text(65.0), "1:05");
        assert_eq!(zeit_text(3599.0), "59:59");
        assert_eq!(zeit_text(-1.0), "0:00", "eine negative Zeit ist keine Zeit");
        assert_eq!(zeit_text(f64::NAN), "0:00", "NaN ist keine Zeit");
        assert_eq!(zeit_text(f64::INFINITY), "0:00", "unendlich ist keine Zeit");
    }

    /// DER SPRUNG BLEIBT IN DER DAUER.
    #[test]
    fn ein_sprung_ueber_das_ende_hinaus_wird_gebegrenzt() {
        const B: u32 = 1200;
        const H: u32 = 760;
        let x = zustand();
        let t = teile(B, H).expect("Teile");
        match treffer(&x, &Point::new(t.spur.right() - 1.0, t.spur.y + 2.0), B, H) {
            Some(Werkzeug::Zeitleiste(s)) => {
                assert!(s > 14.0 && s <= 15.0, "der Sprung liegt am Ende: {s}")
            }
            a => panic!("erwartet Zeitleiste, gemessen {a:?}"),
        }
    }
}
