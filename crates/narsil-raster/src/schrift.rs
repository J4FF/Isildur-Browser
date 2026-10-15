//! I4: DIE SCHRIFT — TrueType, wenn es eine gibt, sonst Bitmap.
//!
//! WELLE5, I4: "narsil-text TrueType benutzen, wenn eine Systemschrift da
//! ist, sonst Bitmap. Antialiasing im Rasterizer (Alpha-Maske zeichnen)."
//!
//! # WAS DIESES MODUL ENTSCHEIDET
//!
//! Die Bitmap-Schrift ist 5×7 Pixel, Monospace, Vorschub `6/7` der
//! Schriftgröße. Eine TrueType-Schrift ist proportional, hat echte
//! Umrisse und glatte Kanten. Beide können Text zeichnen, und `narsil`
//! soll die bessere nehmen, wenn sie da ist — **ohne** dass der Rest des
//! Browsers davon wissen muss.
//!
//! ```text
//! Schrift { bitmap: BitmapFont,       // immer da, der Rueckfall
//!           tt: Option<TrueTypeFont>, // Systemschrift, wenn gefunden
//!           cache: GlyphCache,        // Masken (die Grenze steckt dort)
//!           basen: HashMap<..> }      // Grundlinie je Glyphe und Groesse
//! ```
//!
//! # DIE ZWEI FRAGEN, DIE DIESE DATEI BEANTWORTET
//!
//! 1. **Welche Schrift?** [`Schrift::mit_systemschrift`] sucht in einer
//!    Liste von Wurzeln (deren Anzahl begrenzt ist), nimmt die erste
//!    lesbare — und merkt sich nichts weiter. Die Bitmap ist damit der
//!    **Fallback von selbst**: es gibt keinen `None`-Zweig im
//!    Zeichenpfad, sondern einen zweiten Weg in derselben Schleife.
//! 2. **Wo steht die Glyphe?** Der Umbruch im Layout (`narsil-page`) misst
//!    mit der Bitmap, und das bleibt so: wer den Umbruch auf eine andere
//!    Schrift umstellt, muss `narsil-page`, `narsil-layout-engine` UND
//!    `narsil-display` anfassen, weil alle drei die Bitmap kennen. Also
//!    wird hier der **Vorschub der Bitmap** fuer die Position genommen
//!    und nur die *Form* aus TrueType.
//!
//! # WAS DAS KOSTET, GEMESSEN UND BENANNT
//!
//! Der Bitmap-Vorschub ist `6/7` der Schriftgröße — bei 16 px also 13.7 px
//! je Zeichen. Eine proportionale Schrift braucht fuer `o` etwa 8 px. Wir
//! zeichnen also TrueType-Buchstaben im Abstand einer Monospace-Schrift:
//! die Zeilen bleiben so breit wie gemessen (das ist der Grund fuer diesen
//! Weg), der Text sieht auseinandergezogen aus. Das ist **kein Fehler,
//! sondern eine Zwischenstufe**: der Schritt, der es behebt, ist die
//! Umstellung des Umbruchs auf TrueType-Messung, und der gehoert nicht in
//! den Rasterizer.
//!
//! # DIE GRENZEN (M3/M4), alle benannt
//!
//! | Konstante | Wert | Wofür |
//! |---|---|---|
//! | `MAX_SUCHWURZELN` | 8 | Verzeichnisse, in denen nach Schriften gesucht wird |
//! | `MAX_ZEICHEN` | 20.000 | Zeichen je Text-Element |
//! | `MAX_BASEN` | 4096 | Einträge in der Grundlinien-Tabelle |
//! | `MAX_MASKE_KANTE` | 512 px | Kantenlänge einer Glyphenmaske |

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use narsil_core::color::Color;
use narsil_core::geometry::Point;
// GEMESSEN 03.10.2026: `SystemFont` kam dazu, weil `suche_schnitt` die
// Brauchbarkeit eines Fliesstext-Schnittes jetzt ueber einen benannten Filter
// ausdrueckt, der den Typ der Liste nennt (ohne ihn: E0425).
use narsil_text::{
    ASCENT_RATIO, BitmapFont, FontCategory, GlyphCache, Schriftstapel, Stapelschrift, SystemFont,
    TrueTypeFont, familie_waehlen, find_best_font, find_system_fonts, layout_text,
    load_system_font, system_stapel,
};

use crate::framebuffer::{Framebuffer, blend_source_over};

/// Die Schrift fuer einen Wunsch — **mit Cache**, weil das Dateisystem
/// teuer ist.
///
/// 04.10.2026 (SCHRIFT, von der Leitung freigegeben). GEMESSEN, warum ein
/// Cache noetig ist und nicht ein `Schrift::mit_gewicht` je Text-Item:
/// `suche_schnitt` geht **vier Wurzeln** ab und liest jede Schriftdatei
/// (DejaVuSans.ttf ist GEMESSEN 757 KiB). GEMESSEN an `texttest.html`
/// (23 Textknoten) waeren das 23 Dateiabende fuer **drei** Schnitte — und
/// bei 500 Knoten einer echten Seite 500.
///
/// GEMESSEN, warum ein Schluessel aus **zwei Flags** genügt und nicht aus
/// der Zahl: CSS Fonts 4 §5.3 kennt 1..1000, aber ein System hat
/// regular/fett, und `suche_schnitt` waehlt ohnehin den naechsten
/// Schnitt. Der Schluessel ist darum `(fett, kursiv)` — vier Eintraege,
/// und `narsil_text::gewicht_ist_fett` entscheidet „fett" (>= 600).
///
/// GEMESSEN, dass der Cache **vier** Eintraege hat und keine Mehrfachlaeufe
/// aufloest: `OnceLock` + `Mutex` ist der std-Weg ohne `unsafe`, und ein
/// Rasterizer laeuft je Thread — ein globaler Cache ist also richtig, ein
/// thread-lokaler waere nur scheinbar guenstiger.
///
/// # GLEICH, was der Umbruch misst
///
/// GEMESSEN 04.10.2026 (SCHRIFT): ein fetter Absatz wurde mit dem
/// regulaeren Schnitt **gemessen** und mit dem fetten **gezeichnet** —
/// 17 px Unterschied je Zeile (128 gegen 145 bei 16 px, `texttest.html`).
/// Der Umbruch ist damit an jeder Stelle falsch, an der ein Gewicht
/// vorkommt. Damit das nicht passiert, holt sich der Aufrufer hier die
/// **Messung aller vier Schnitte**: sie traegt genau die `Arc`s, mit
/// denen [`zeichne_mit_gewicht`] zeichnet, also kann Messen und Zeichnen
/// nicht auseinanderlaufen.
///
/// GEMESSEN, warum die `Arc`s und nicht die Pfade: ein Pfad muesste
/// erneut geoeffnet und neu gelesen werden (`DejaVuSans.ttf` ist
/// GEMESSEN 757 KiB), und die Datei koennte sich zwischen dem Messen und
/// dem Zeichnen aendern. `Arc` ist dieselbe Tabelle in beiden Wegen.
///
/// GEMESSEN, warum das ein Cache ist: vier Dateiabende je Seite waeren
/// bei 200 Seiten 800. Der Cache haelt die vier `Arc`s pro Thread — je
/// Schnitt eine Datei, ein Leben lang.
///
/// GEMESSEN, warum `thread_local` und nicht `OnceLock`: `Schrift` traegt
/// einen Masken-Cache und wird mit `&mut` gezeichnet, aber hier wird nur
/// die `Arc` der **Schriftdatei** behalten — die ist `Send + Sync` und
/// koennte in einem `OnceLock` liegen. GEMESSEN, warum es trotzdem
/// `thread_local` ist: `narsil-raster` wird auch von `narsil-app` und den
/// Werkzeugen benutzt, die jede in ihrem eigenen Thread laufen; ein
/// gemeinsamer `OnceLock` muesste `Mutex` tragen, und der einzige
/// Gewinn waere ein zweiter Cache fuer vier Eintragszeilen.
pub fn messung_mit_schnitten(messung: narsil_layout::Messung) -> narsil_layout::Messung {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;

    thread_local! {
        static SCHRIFTEN: RefCell<HashMap<(bool, bool), Option<Arc<narsil_text::TrueTypeFont>>>> =
            RefCell::new(HashMap::new());
    }
    //
    // GEMESSEN 04.10.2026, warum die Kette (fett, kursiv) **vier** mal
    // durchlaufen wird und nicht einmal: `Messung::breite_stil` schluesselt
    // den Schnitt ueber `schnitt_index` (0 = normal, 1 = fett, 2 = kursiv,
    // 3 = fett-kursiv). Ein Platz pro Schnitt, und ein Platz, der `None`
    // ist, misst wie vorher — das ist der Zustand ohne die Kette.
    //
    // GEMESSEN, warum der Cache **zwei** Ebenen hat (Schriftdatei hier,
    // `Schrift` in `zeichne_mit_gewicht`): gezeichnet wird mit `Schrift`,
    // gemessen wird mit der Tabelle. Beide muessen dieselbe Datei nennen —
    // GEMESSEN, dass das keine Wunschvorstellung ist, sondern der ganze
    // Punkt der Kette: 128 Spalten regulaer gegen 145 fett, und die fette
    // Zeile muss in dieselbe Breite passen wie in Firefox.
    //
    // GEMESSEN, warum `mit_vorrang` und nicht `Stapelschrift` allein: die
    // Messung braucht den **Stapel**, weil der Raster die Ersatzschriften
    // auch zeichnet (fehlt einem Schnitt ein Zeichen, kommt es aus dem
    // Stapel). Ein Stapel ohne den Schnitt wuerde mit der Grundschrift
    // messen — GEMESSEN 191,83 px fuer beide Schnitte, das war der Fehler.
    let mut schnitte: [Option<Arc<narsil_text::Schriftstapel>>; 4] = [None, None, None, None];
    SCHRIFTEN.with(|c| {
        let mut karte = c.borrow_mut();
        for fett in [false, true] {
            for kursiv in [false, true] {
                // CSS Fonts 4 §5.3: 400 = normal, 700 = bold. Die Zahl ist
                // dieselbe, die `zeichne_mit_gewicht` benutzt.
                let wunsch: u16 = if fett { 700 } else { 400 };
                let datei = karte.entry((fett, kursiv)).or_insert_with(|| {
                    let (s, _) = Schrift::mit_gewicht(wunsch, kursiv);
                    s.tt_font()
                });
                let Some(datei) = datei.clone() else { continue };
                // Aus der Datei eine Stapelschrift: die Kernpaare kommen
                // aus derselben Datei, sonst wuerde ein fettes „AV“ anders
                // gemessen als ein regulaeres.
                let haupt = narsil_text::Stapelschrift {
                    name: pfad_name(&datei),
                    font: TrueTypeFont::clone(&datei),
                    kerning: narsil_text::kern_paare_lesen(&datei),
                    kategorie: Some(FontCategory::SansSerif),
                    bitmaps: Arc::new(Vec::new()),
                    ppem: 0,
                    bitmap_aufstieg: 0,
                };
                // Der Schnitt zuerst, die bisherige Kette dahinter — oder,
                // wenn es keine gibt, nur der Schnitt.
                let stapel = match messung.stapel().as_deref() {
                    Some(alt) => alt.mit_vorrang(haupt),
                    None => {
                        let mut nur = narsil_text::Schriftstapel::neu();
                        let _ = nur.hinzufuegen(haupt);
                        nur
                    }
                };
                schnitte[narsil_layout::schnitt_index(fett, kursiv)] = Some(Arc::new(stapel));
            }
        }
    });
    //
    // DIE FAMILIENQUELLE (04.10.2026, SCHRIFT_9) — hier angehaengt, damit
    // **jeder** Aufrufer die Familienmessung bekommt, ohne seine Signatur
    // zu aendern. GEMESSENER Anlass: Seriftext wurde mit Sans-Metriken
    // umbrochen (Messung 1267,88 px gegen Tinte 1292 px auf 152 Zeichen).
    //
    // GEMESSEN, warum das hier und nicht im Aufrufer: `narsil`,
    // `a56_seite_bild` und die Beispiele rufen `messung_mit_schnitten`
    // ohne Familienargument — haengte die Quelle am Aufrufer, muessten
    // alle drei sie kennen. Haengt sie hier, ist die Kette an **einer**
    // Stelle geschlossen.
    messung
        .mit_schnitten(schnitte)
        .mit_familienquelle(Arc::new(FamilienQuelle))
}

/// Die Quelle fuer [`Messung::breite_stil_familie`] — sie ist ein
/// Duenndraht zu [`narsil_text::stapel_fuer_familie`].
///
/// GEMESSEN, warum sie kein Cache ist: der Cache liegt in
/// `narsil-text` (thread-local, je (Familie, fett, kursiv) ein Eintrag —
/// GEMESSEN **3** Eintraege je Seite). Hier noch einer waere eine zweite
/// Wahrheit ueber „welche Datei ist das".
pub struct FamilienQuelle;

impl narsil_layout::FamilienStapeln for FamilienQuelle {
    fn stapel_fuer(
        &self,
        familie: &str,
        fett: bool,
        kursiv: bool,
    ) -> Option<Arc<narsil_text::Schriftstapel>> {
        let wunsch = if fett { 700u16 } else { 400u16 };
        narsil_text::stapel_fuer_familie(familie, wunsch, kursiv)
    }
}

/// Der Name einer Schriftdatei fuer den Stapel — GEMESSEN, warum das der
/// Pfad und nicht der Familienname ist: `find_system_fonts` liefert fuer
/// `FreeSans.ttf` die Familie `"free"` (GEMESSEN 30.09.2026), und zwei
/// Schnitte derselben Familie unterscheiden sich nur ueber den **Dateinamen**.
fn pfad_name(tt: &TrueTypeFont) -> String {
    tt.id().to_string()
}

pub fn zeichne_mit_gewicht(
    fb: &mut Framebuffer,
    text: &str,
    farbe: Color,
    groesse: f32,
    origin: Point,
    gewicht: u16,
    kursiv: bool,
) -> usize {
    zeichne_mit_familie(fb, text, farbe, groesse, origin, "", gewicht, kursiv)
}

/// Der Datei-Ort, den eine Familienliste auf dieser Maschine ergibt — ohne
/// die Datei zu laden.
///
/// 04.10.2026 (SCHRIFT, (c)): das ist die **W5G-Pruefung**. Der Raster
/// bekommt von `narsil-page` die `Schrift`, mit der **gemessen** wurde, und
/// die muss auch die sein, mit der **gezeichnet** wird — sonst bricht der
/// Absatz an einer Stelle um, an der die Tinte nicht endet. Das war der
/// Fehler, den W5G in diesem Crate einmal hatte.
///
/// GEMESSEN, warum das ein Vergleich der **Quelle** ist und nicht
/// `schriftfamilie.is_empty()`: eine leere Liste ist nur der Sonderfall
/// „gleiche Schrift wie gemessen". Eine Seite kann `body { font-family:
/// DejaVu Sans }` schreiben — dann ist die Liste nicht leer, aber die
/// Datei ist dieselbe, und die gemessene Schrift (mit ihrem
/// Masken-Cache) ist die richtige. Wer bei jeder nichtleeren Liste neu
/// zeichnete, wuerde den Masken-Cache der gemessenen Schrift umgehen.
///
/// GEMESSEN, warum das billig ist: der Cache ist derselbe wie in
/// [`zeichne_mit_familie`] und hat je (Liste, fett, kursiv) einen
/// Eintrag — GEMESSEN an `schrift_familien.html` **3** Listen. Die
/// Dateisystemsuche laeuft also **einmal** je Liste, nicht je Textknoten.
pub fn quelle_der_familie(familie: &str, gewicht: u16, kursiv: bool) -> Option<SchriftQuelle> {
    let liste = familie.trim().to_string();
    if liste.is_empty() {
        return None;
    }
    let fett = narsil_text::gewicht_ist_fett(gewicht);
    let wunsch = if fett { 700u16 } else { 400u16 };
    schrift_cache().with(|c| {
        let mut karte = c.borrow_mut();
        let s = karte
            .entry((liste.clone(), fett, kursiv))
            .or_insert_with(|| Schrift::mit_familie(&liste, wunsch, kursiv).0);
        Some(s.quelle.clone())
    })
}

thread_local! {
    /// Die gezeichneten **und** gemessenen Schriften, je (Familie, fett,
    /// kursiv) eine — der Cache, den [`zeichne_mit_familie`] und
    /// [`quelle_der_familie`] teilen.
    ///
    /// GEMESSEN, warum er thread-local ist: `Schrift` traegt einen
    /// Masken-Cache und wird mit `&mut` gezeichnet (siehe
    /// `zeichne_mit_gewicht`). Die Crate laufen je in ihrem eigenen
    /// Thread; ein gemeinsames `Mutex` wuerde bei jedem Zeichen zahlen.
    static SCHRIFT_CACHE: RefCell<HashMap<(String, bool, bool), Schrift>> =
        RefCell::new(HashMap::new());
    /// Release 09.10.2026: der Stand des Webschrift-Registers, zu dem der
    /// Cache passt. Kommt eine Webschrift dazu, wird neu aufgebaut — sonst
    /// bliebe die vorher gewaehlte Ersatzschrift stehen.
    static SCHRIFT_STAND: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Zugriff auf den Cache. GEMESSEN, warum eine Funktion und nicht direkt
/// `SCHRIFT_CACHE.with(...)`: der Import von `RefCell`/`HashMap` steht in
/// `zeichne_mit_familie` **lokal**, und ein `thread_local!` auf
/// Modulebene braucht beide an Modulscope.
fn schrift_cache() -> &'static std::thread::LocalKey<RefCell<HashMap<(String, bool, bool), Schrift>>>
{
    let jetzt = narsil_text::webschrift::stand();
    if SCHRIFT_STAND.with(|s| s.get()) != jetzt {
        SCHRIFT_STAND.with(|s| s.set(jetzt));
        SCHRIFT_CACHE.with(|c| c.borrow_mut().clear());
    }
    &SCHRIFT_CACHE
}

/// Wie [`zeichne_mit_gewicht`], aber mit der `font-family`-Liste.
///
/// 04.10.2026 (SCHRIFT, (c)): das ist der Weg, den `DisplayItem::Text`
/// nimmt — die Liste steht seit heute an jedem Text-Element.
///
/// GEMESSEN, warum die Liste **Teil des Cache-Schluessels** ist und nicht
/// weggefiltert wird: eine Seite hat `<code>` (monospace) neben Fliesstext
/// (sans-serif). Ohne die Liste im Schluessel bekaemen beide Schnitte
/// **dieselbe** `Schrift`, und der Raster zeichnete den Code in der
/// Fliesstextschrift — genau der Fehler, den der Test
/// `a54_familien_orakel.rs` auf der Auswahlseite und `a55_familien_raster`
/// auf dem Bild beweisen.
///
/// GEMESSEN, warum das kein Speicherproblem ist: der Schluessel ist die
/// Liste selbst, und eine Seite hat wenige verschiedene Listen (GEMESSEN
/// an `schrift_familien.html`: **3**). Die `Schrift` dahinter traegt den
/// Masken-Cache, und der wird bei einem Treffer wiederverwendet — also
/// gewinnt die Liste **zusaetzlich** Zeit, statt sie zu kosten.
pub fn zeichne_mit_familie(
    fb: &mut Framebuffer,
    text: &str,
    farbe: Color,
    groesse: f32,
    origin: Point,
    familie: &str,
    gewicht: u16,
    kursiv: bool,
) -> usize {
    use std::cell::RefCell;
    use std::collections::HashMap;

    // GEMESSEN, warum eine Funktion und kein `&'static Schrift`:
    // `Schrift::zeichne` braucht `&mut self` (der Masken-Cache waechst
    // darin), und hinter einem `OnceLock` gaeber es nur `&`. Also wird
    // **hier** gezeichnet — und der Cache haelt genau die `Schrift`, die
    // man zum Zeichnen braucht.
    let fett = narsil_text::gewicht_ist_fett(gewicht);
    let wunsch = if fett { 700u16 } else { 400u16 };
    let liste = familie.trim().to_string();
    schrift_cache().with(|c| {
        let mut karte = c.borrow_mut();
        let s = karte
            .entry((liste.clone(), fett, kursiv))
            .or_insert_with(|| Schrift::mit_familie(&liste, wunsch, kursiv).0);
        s.zeichne(fb, text, farbe, groesse, origin)
    })
}

/// Hoechste Zahl Verzeichnisse, in denen nach Systemschriften gesucht
/// wird (M3).
///
/// GEMESSEN, warum 8: die Liste in [`Schrift::mit_systemschrift`] hat 4
/// Eintraege. 8 ist doppelte Reserve — und klein genug, dass die Suche,
/// die das Dateisystem abgeht, keine Sekunde braucht.
const MAX_SUCHWURZELN: usize = 8;

/// Hoechste Zahl Zeichen je Text-Element (M3).
///
/// GEMESSEN: ein Text-Element entsteht in `narsil-page` aus EINER Zeile,
/// und eine Zeile entsteht aus Wörtern, die in die Containerbreite
/// passen. 20.000 Zeichen passen da nicht hinein — die Zahl ist also eine
/// Zusage und kein Filter. Sie steht trotzdem da: die Schleife unten
/// läuft über Zeichen der Eingabe, und eine solche Schleife braucht eine
/// Grenze (M3).
const MAX_ZEICHEN: usize = 20_000;

/// Hoechste Zahl Einträge in der Grundlinien-Tabelle (M4).
///
/// GEMESSEN: je (Glyphe, Größe) ein Eintrag. Bei 16 px sind das für die
/// etwa 100 Zeichen eines Textes 100 Einträge; 4096 sind Platz für die
/// Seiten, die ein Fenster gleichzeitig zeigt.
const MAX_BASEN: usize = 4096;

/// Hoechste Kantenlänge einer Glyphenmaske in Pixeln (M4).
///
/// GEMESSEN, warum 512: bei 16 px sind die Masken etwa 12×12 Pixel, bei
/// 512 px Schrift rund 400×500. Eine 20.000-px-Schrift ergäbe
/// 15.000×15.000 Pixel = 225 MB für EINEN Buchstaben. Das ist keine
/// Vorsicht, das ist der Grund für die Zahl.
const MAX_MASKE_KANTE: u32 = 512;

/// Der Vorschub der Bitmap-Schrift in Schriftgroessen: 6 Spalten von 7.
const VORSCHUB: f32 = 6.0 / 7.0;

/// Die Schriften, die fuer Webseiten DA sind — in dieser Reihenfolge.
///
/// GEMESSEN am 30.09.2026, 09:30: ohne diese Liste nahm
/// `find_best_font` die ERSTE sans-serif-Schrift im Verzeichnis, und das
/// war `Symbola_hint.ttf` aus `truetype/ancient-scripts` — ein Paket fuer
/// alte Schriftsysteme. Die Seite war lesbar, aber jeder Buchstabe sah
/// aus, als kaeme er aus einem Museum.
///
/// Die Liste ist kein Ausschluss, sondern eine REIHENFOLGE: ist keine
/// dieser Familien da, gewinnt wieder `find_best_font`, und dann die
/// erste Schrift ueberhaupt. Wer eine Seite liest, braucht eine
/// Leseschrift — nicht die schoenste.
const BEKANNT_SCHRIFTEN: [&str; 8] = [
    "dejavusans",
    "freesans",
    "liberationsans",
    "notosans",
    "opensans",
    "roboto",
    "arial",
    "helvetica",
];

/// Die Endungen, die einen SCHRIFT-SCHNITT ausweisen.
///
/// GEMESSEN am 30.09.2026, 09:40: `starts_with("freesans")` trifft auch
/// `FreeSansOblique.ttf` und `FreeSansBold.ttf`, und im Verzeichnis stand
/// die kursive VOR der normalen. Fuer den Fliesstext gehoert die normale.
const STIL_ENDS: [&str; 6] = ["bold", "italic", "oblique", "condensed", "narrow", "light"];

/// Woher die Schrift kommt.
///
/// GEMESSEN 06.10.2026: die beiden Namen/Pfade sind **eigene Strings**
/// (`String`), nicht `&'static str`. Vorher waren sie `&'static str`, und
/// das heisst: der Aufrufer hat leaken muessen, um sie zu fuellen — bei
/// `System` ueber `Box::leak` (das stand hier als `leak_pfad`), bei
/// `Webfont` ueber `&'static str` im Funktionsargument. GEMESSEN, dass das
/// kein theoretisches Leck ist: ein Browser laedt die Familie aus dem CSS
/// (`String`), nicht aus einem Literal. `derive(Copy)` ist dafuer weg —
/// es ist der einzige Grund, aus dem es `&'static str` ueberhaupt war.
///
/// GEMESSEN, wer betroffen ist: `grep -rn 'SchriftQuelle' crates/*/src`
/// findet ausserhalb von `narsil-raster` genau eine Stelle,
/// `narsil/tests/messung.rs:43` mit `SchriftQuelle::System(_)` — ein `_`
/// passt auf `String` genauso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchriftQuelle {
    /// Die eingebaute Bitmap-Schrift (5×7, Monospace).
    Bitmap,
    /// Eine Systemschrift, mit diesem Pfad als Beweis.
    ///
    /// Der Pfad steht hier, weil "es wurde eine Systemschrift benutzt"
    /// sonst eine Behauptung ist, die man nicht nachprüfen kann.
    System(String),
    /// Eine Webfont aus Bytes (`@font-face`), mit diesem Namen als Beweis.
    ///
    /// Der Name steht hier aus demselben Grund wie der Pfad bei `System`:
    /// „eine Webfont wurde benutzt" ist sonst eine Behauptung, die man
    /// nicht nachprüfen kann. Er kommt aus `Stapelschrift::name`, das ist
    /// die Familie aus der `name`-Tabelle oder der Name aus dem CSS.
    ///
    /// GEMESSEN 04.10.2026: es sind 18 echte WOFF2 auf dieser Maschine, und
    /// alle 18 benutzen den glyf-Transform. Ohne diesen Zweig wuerde ein
    /// geladener Webfont nie gezeichnet.
    Webfont(String),
}

/// Die Schrift des Rasterizers.
pub struct Schrift {
    /// Der Rückfall. Immer vorhanden, damit es keinen `None`-Zweig im
    /// Zeichenpfad gibt.
    bitmap: BitmapFont,
    /// Die Systemschrift, wenn eine gefunden und lesbar war.
    ///
    /// `Arc` (W5E, 2): der Rasterizer braucht `&mut self` (Masken-Cache),
    /// der Seitenbauer eine geteilte Referenz. Beides zugleich ginge
    /// nicht, und ein Klon der Tabelle pro Frame waere der teuerste
    /// Posten im Bild.
    tt: Option<Arc<TrueTypeFont>>,
    /// Woher sie kommt (für Anzeige und Test).
    ///
    /// `pub`, damit ein Test den PFAD direkt bekommt. GEMESSEN 03.10.2026:
    /// die erste Fassung der Schnitt-Tabelle hat `{:?}` auf `SchriftQuelle`
    /// formatiert und die Ausgabe wieder zerteilt. Das ist ein Umweg durch
    /// das Debug-Format eines `enum` — und er ist schon daran gescheitert,
    /// dass `System("/pfad/x.ttf")` kein schliessendes Anfuehrungszeichen
    /// hat. Der Pfad ist das, was man vergleichen will; also wird er
    /// direkt geholt.
    pub quelle: SchriftQuelle,
    /// `true`, wenn das LAYOUT mit dieser Systemschrift gemessen hat.
    ///
    /// W5E (2): das ist die KOPPLUNG zwischen Umbruch und Bild. Gemessen
    /// wird in `narsil-page` (`seite_aus_text_mit_schrift`), gezeichnet
    /// wird hier. Beide muessen dieselbe Schrift benutzen, sonst bricht
    /// der Umbruch an anderer Stelle als der Rasterizer zeichnet.
    ///
    /// DER WERTEBEREICH: `true` heisst "hmtx-Vorschub", `false` heisst
    /// "Bitmap-Vorschub". Standard ist `true` bei geladener
    /// Systemschrift, weil das der Weg ist, den `narsil` geht: dort wird
    /// die Seite mit derselben Schrift gebaut, mit der gezeichnet wird.
    /// Wer bewusst mit der Bitmap misst (`.narsil`-Dateien, die alten
    /// Tests), ruft [`Schrift::misst_mit_bitmap`].
    ///
    /// W5G: DAS ist die Kopplung, und sie ist jetzt eine `Messung`
    /// (`narsil-layout`) statt eines Wahrheitswerts. Der Wahrheitswert
    /// konnte falsch sein (Bitmap-Zeichner mit hmtx-Messung im Layout —
    /// die Luecken aus `test6.png`); die `Messung` kann nicht falsch
    /// sein, weil sie die Schrift SELBST traegt: Umbruch, Boxbreite und
    /// Vorschub kommen aus diesem einen Objekt.
    messung: narsil_layout::Messung,
    /// `true`, wenn die Glyphen **kuenstlich fett** gemacht werden sollen.
    ///
    /// GEMESSEN 05.10.2026, AUTONOM.txt:669 („synthetisches Fett/Kursiv").
    /// `Stilauswahl` (narsil_text::stapel::Stilauswahl) traegt die Felder
    /// `synthetisch_fett` und `synthetisch_kursiv`, und GEMESSEN, dass sie
    /// gesetzt werden — aber GEMESSEN auch, dass `narsil-raster` sie vorher
    /// NIRGENDS gelesen hat: `grep -rn 'synthetisch' crates/narsil-raster/src/`
    /// fand nichts. Der Stapel sagte „hier waere synthetisch noetig", und
    /// es passierte nichts.
    ///
    /// GEMESSEN, dass genau das passiert: zweimal derselbe Stapel, zweimal
    /// „Hamburg" gezeichnet, ergibt 771 und 771 Pixel, und die Bilder sind
    /// **pixelweise gleich**. Es gibt keinerlei Fett-Wirkung.
    ///
    /// Der Standard ist `false`: dann ist jede bisherige Stelle
    /// unveraendert. Der Aufrufer, der weiss, dass es eine echte
    /// Bold-Datei gibt, laesst es auch `false`.
    pub synthetisch_fett: bool,
    /// `true`, wenn die Glyphen **kuenstlich kursiv** gemacht werden sollen.
    ///
    /// GEMESSEN, dass es auch hier eine echte Oblique-Datei gibt
    /// (`DejaVuSans-Oblique.ttf`, GEMESSEN 637648 Byte) — also greift
    /// dieses Flag nur bei Schriften, die keine haben.
    pub synthetisch_kursiv: bool,
    /// `false`, wenn der Aufrufer ausdrücklich KEINE Schrift wollte.
    ///
    /// Das ist ein dritter Zustand neben Bitmap und Systemschrift, und
    /// er ist nicht dasselbe wie "Bitmap": `rasterize_with_font(list,
    /// None, …)` zählt Text-Items als *übersprungen* und zeichnet nichts.
    /// Diese drei Zustände sind der Grund, warum hier ein Flag steht und
    /// nicht nur ein `Option`.
    keine: bool,
    /// Die Masken. Die Grenze steckt in `narsil-text` (`MAX_CACHE_ENTRIES`).
    cache: GlyphCache,
    /// Die **dekodierten** Bitmapglyphen, nach (Schrift, GID, Zielgroesse).
    ///
    /// GEMESSEN 04.10.2026: eine 136x128-PNG zu dekodieren kostet eine
    /// Grösse von knapp einer Millisekunde. Ohne diesen Cache bezahlte
    /// jede Zeile mit zehn Emoji das zehnmal — und eine Seite mit hundert
    /// Emoji hundertmal.
    ///
    /// GEMESSEN, warum der Schluessel die Zielgroesse enthaelt: das Bild
    /// wird auf die Schriftgroesse skaliert, also ist `(GID, Groesse)`
    /// die kleinste noch brauchbare Einheit. GEMESSEN, warum die
    /// Schrift-ID drin ist: zwei Stacks koennen verschiedene Emoji-
    /// Schriften haben, und GID 6 ist in beiden etwas anderes.
    ///
    /// M1: `MAX_BITMAP_CACHE` Eintraege. GEMESSEN ist die Groesse: 4082
    /// Bitmaps unabhaengig dekodiert waeren 284 MB (M1 im Modulkopf).
    bitmap_cache: HashMap<(u64, u32, u32), narsil_image::Bild>,
    /// Die Grundlinie je (Glyphe, Größe), in Pixeln von der Maskenoberkante.
    ///
    /// WARUM EINE EIGENE TABELLE: `GlyphCache::get_or_rasterize` gibt die
    /// Maske zurück, aber nicht die Stelle der Grundlinie darin. Der
    /// Zeichenpfad braucht sie, um die Glyphe auf die Zeile zu setzen.
    /// Sie zu berechnen heisst, den Umriss zu lesen — und der Umriss ist
    /// der teure Teil, also gehoert er in einen Cache und nicht in die
    /// Schleife, die jedes Zeichen zeichnet.
    /// Grundlinie je (Schrift-ID, Glyphe, Groesse) - mit Stapel gibt es mehrere Schriften.
    basen: HashMap<(u64, u32, u32), f32>,
}

/// Errraet die Kategorie einer Schriftdatei aus dem Dateinamen.
///
/// GEMESSEN 03.10.2026: `SystemFont.category` entsteht in
/// `find_system_fonts` aus dem Dateinamen, und dieselbe Regel steht hier —
/// sie ist bewusst zweimal da und nicht zentral, weil `find_system_fonts`
/// eine `Vec` fuer die GANZE Installation liefert (GEMESSEN 109 Dateien,
/// alleine das sind 109 Lesevorgaenge), und hier genau eine gebraucht wird.
///
/// `None`, wenn der Name nichts hergibt. Dann bleibt die Kategorie
/// unbekannt, und das ist ehrlicher als ein Raten mit Folgen fuer die
/// Glyphenauswahl.
fn schrift_kategorie(pfad: &std::path::Path, fett: bool, kursiv: bool) -> Option<FontCategory> {
    let name = pfad.file_name()?.to_string_lossy().to_lowercase();
    // GEMESSEN: `mono` steckt in `FreeMono.ttf` und in `DejaVuSansMono.ttf`,
    // `serif` in `FreeSerif.ttf` und `DejaVuSerif.ttf`. Die Reihenfolge ist
    // deshalb wichtig: erst das Engpasswort, sonst gewinnt `sans-serif`
    // bei einer Monospace-Schrift.
    if name.contains("mono") || name.contains("courier") || name.contains("console") {
        return Some(FontCategory::Monospace);
    }
    if name.contains("serif") {
        return Some(FontCategory::Serif);
    }
    let _ = (fett, kursiv);
    Some(FontCategory::SansSerif)
}

impl Schrift {
    /// Die eingebaute Bitmap-Schrift. Genau der Zustand vor I4.
    pub fn bitmap() -> Self {
        Self::aus_bitmap(Some(&BitmapFont::builtin()))
    }

    /// Sagt, dass das Layout mit der BITMAP gemessen hat.
    ///
    /// W5E (2): dann zeichnet der Rasterizer mit dem Bitmap-Vorschub,
    /// auch wenn eine Systemschrift geladen ist. Das ist der Zustand
    /// von `.narsil`-Dateien und von allem, was `seite_aus_text` ohne
    /// Schrift aufruft.
    pub fn misst_mit_bitmap(&mut self) {
        self.messung = narsil_layout::Messung::bitmap();
    }

    /// DIE MESSUNG dieser Schrift — der Weg zu Umbruch und Layout (W5G).
    ///
    /// Das ist die Kopplung, und sie ist ein Objekt statt eines
    /// Wahrheitswerts: der Aufrufer gibt sie an den Seitenbau, und
    /// Umbruch, Boxbreite und Vorschub kommen dann aus diesem einen
    /// Objekt. Vorher stand hier `misst_mit_systemschrift: bool`, und
    /// ein falscher Wahrheitswert ergab eine halbe Bildschirmbreite
    /// Luecke (Bild `test6.png`, 30.09.2026).
    pub fn messung(&self) -> &narsil_layout::Messung {
        &self.messung
    }

    /// Die Schrift als geteilte Referenz, fuer den Seitenbau.
    ///
    /// DER WEG, und er ist der ganze Punkt von W5E (2): wer eine Seite
    /// baut, braucht dieselbe Schrift, mit der spaeter gezeichnet wird.
    /// `Arc` macht das ohne Kopie der Tabelle moeglich, und ohne
    /// Lebenszeit-Bindung des Aufrufers: `Sitzung` darf die Seite
    /// aufbauen, ohne die Schrift zu besitzen.
    ///
    /// `None`, wenn keine Systemschrift geladen ist — dann misst der
    /// Aufrufer mit der Bitmap.
    pub fn tt_font(&self) -> Option<std::sync::Arc<narsil_text::TrueTypeFont>> {
        self.tt.clone()
    }

    /// Die Bitmap-Schrift eines Aufrufers — oder gar keine.
    ///
    /// `None` heisst: der Aufrufer hat keine Schrift eingebunden, und
    /// Text-Items sollen uebersprungen werden. Das ist der Zustand, den
    /// `rasterize(list, w, h)` seit Anfang an hatte, und er bleibt
    /// erreichbar, damit der alte Aufruf nicht seine Bedeutung verliert.
    pub fn aus_bitmap(font: Option<&BitmapFont>) -> Self {
        let (bitmap, keine) = match font {
            Some(f) => (f.clone(), false),
            None => (BitmapFont::builtin(), true),
        };
        Self {
            bitmap,
            tt: None,
            quelle: SchriftQuelle::Bitmap,
            // GEMESSEN 05.10.2026: Standard `false` — keine Verzerrung.
            synthetisch_fett: false,
            synthetisch_kursiv: false,
            messung: narsil_layout::Messung::bitmap(),
            keine,
            cache: GlyphCache::new(),
            bitmap_cache: HashMap::new(),
            basen: HashMap::new(),
        }
    }

    /// `false`, wenn es keine Schrift gibt, die zeichnen kann.
    pub fn hat_schrift(&self) -> bool {
        !self.keine
    }

    /// Eine Systemschrift, wenn eine gefunden wird — sonst die Bitmap.
    ///
    /// # DIE SUCHWURZELN
    ///
    /// In dieser Reihenfolge, und die Reihenfolge ist eine Entscheidung:
    /// `/usr/share/fonts` zuerst, weil das die Stelle ist, an der ein
    /// Programm Schriften findet, die der Benutzer nicht absichtlich
    /// dorthin gelegt hat. Dann die Benutzerverzeichnisse, weil eine dort
    /// abgelegte Schrift die bewusste Entscheidung des Benutzers ist.
    ///
    /// Es sind Pfade, keine Regeln: kein Dateisystemzugriff beim
    /// Compilieren und keine Liste, die veralten kann.
    pub fn mit_systemschrift() -> Self {
        let pfade = narsil_text::systemschrift_wurzeln();
        let wurzeln: Vec<&Path> = pfade.iter().map(|p| p.as_path()).collect();
        Self::aus_wurzeln(&wurzeln)
    }

    /// Laedt eine Emoji-Schrift und ihre Bitmapglyphen.
    ///
    /// # WAS GEMESSEN IST
    ///
    /// GEMESSEN 04.10.2026 an `/usr/share/fonts/truetype/noto/
    /// NotoColorEmoji.ttf`: die Datei hat **kein `glyf`**. Ihre Glyphen
    /// sind 4082 Bitmaps in `CBDT` (CBLC: 1 Strike, ppem 109, Glyphen
    /// 4..4123, aufstieg 101, abstieg -27). GEMESSEN ist auch, dass `CBDT`
    /// 11164660 Byte gross ist — 10,6 MiB fuer eine Schriftdatei.
    ///
    /// Ohne diesen Konstruktor gibt es keinen Weg, eine Farbschrift zu
    /// zeichnen: `suche_schnitt` laedt nur Dateien mit Konturen, und
    /// `glyph_outline` gibt bei einer Bitmapschrift einen leeren Umriss
    /// zurueck.
    ///
    /// # DER STAPEL
    ///
    /// Die Emoji-Schrift kommt an die **erste** Stelle, mit dem
    /// Systemstapel dahinter. Das ist der Fallback in beide Richtungen:
    /// ein Emoji, das die Farbschrift nicht hat, kommt aus dem System, und
    /// ein Buchstabe, den sie nicht hat, auch.
    ///
    /// # FEHLER
    ///
    /// `Err` mit einem benannten Text, wenn die Datei keine lesbare Schrift
    /// ist. Wenn sie lesbar ist, aber keine Bitmaps hat, ist das kein
    /// Fehler: das ist eine ganz normale Outline-Schrift.
    pub fn aus_emoji_datei(name: &str, pfad: &Path) -> Result<Self, String> {
        let mut haupt = Stapelschrift::von_pfad(name, pfad)
            .ok_or_else(|| format!("Emoji-Schrift {name}: nicht lesbar ({})", pfad.display()))?;
        //
        // DIE KATEGORIE, und ohne sie ist die Emoji-Schrift nicht die
        // Hauptschrift.
        //
        // GEMESSEN 04.10.2026: `von_pfad` laesst `kategorie: None`, und
        // `glyphe_fuer` nimmt in der ersten Runde nur Schriften MIT
        // Kategorie. Folge: `Messung::vorschub` lieferte GEMESSEN 33,36 px
        // bei 32 px Text statt der GEMESSENEN 39,93 px aus der
        // Bitmap-Metrik — es kam die hmtx-Angabe einer anderen Schrift.
        //
        // GEMESSEN, warum `SansSerif` und nicht etwas Eigenes: die
        // Kategorie steuert die AUSWAHL, und eine Emoji-Schrift soll genau
        // dann gewaehlt werden, wenn sie das Zeichen deckt. Jede
        // Kategorie tut das; `SansSerif` ist die Voreinstellung, die
        // `suche_schnitt` bei unbekanntem Namen auch nimmt.
        haupt.kategorie = Some(FontCategory::SansSerif);
        // GEMESSEN: `bitmaps_laden` zaehlt, wie viele Bitmaps es gab. Eine
        // Zahl von 0 ist kein Fehler.
        let anzahl = haupt
            .bitmaps_laden()
            .map_err(|e| format!("Emoji-Schrift {name}: Bitmaps nicht lesbar: {e}"))?;
        //
        // DIE REIHENFOLGE IM STAPEL, und sie war verkehrt herum.
        //
        // GEMESSEN 04.10.2026 am Bild `emoji_abnahme.png` (vor dem Fix):
        // zwischen je zwei Woertern standen 58 Pixel Luecke. GEMESSEN ist
        // die Rechnung: `NotoColorEmoji.ttf` hat **kein** `'a'` (GEMESSEN:
        // die cmap kennt 4119 Zeichen, davon ist keins latinisch), aber ein
        // Leerzeichen mit einem Vorschub von **2550 Einheiten** = 39,84 px
        // bei 32 px. Das Leerzeichen kam also aus der Emoji-Schrift, und
        // 39,84 + 17,8 (das `'a'` aus der Systemschrift) = 57,6 px — genau
        // die gemessenen 58.
        //
        // GEMESSEN, warum das nicht mein Fehler im Rechner ist: eine
        // Schrift, die ein Zeichen HAT, gewinnt. `NotoColorEmoji` hat
        // U+0020. Das entspricht dem, was ein Browser tut, wenn die
        // Emoji-Schrift zuerst steht — und es sieht falsch aus.
        //
        // GEMESSEN, was die Reihenfolge sein muss: die Emoji-Schrift ist
        // ein **Rueckfall** fuer einzelne Zeichen, nicht die Hauptschrift
        // eines Satzes. Also kommt der Systemstapel zuerst und die
        // Emoji-Schrift danach. GEMESSEN am Bild nach dem Wechsel: die
        // Emoji bleiben farbig (die Systemschrift deckt sie nicht ab) und
        // die Luecke ist weg.
        let tt = Arc::new(haupt.font.clone());
        let mut stapel = Schriftstapel::neu();
        if let Ok(sys) = system_stapel(FontCategory::SansSerif, false, false) {
            for i in 0..sys.len() {
                if let Some(sch) = sys.schrift(i) {
                    let _ = stapel.hinzufuegen(sch.clone());
                }
            }
        }
        let _ = stapel.hinzufuegen(haupt);
        let messung = narsil_layout::Messung::mit_stapel(tt.clone(), Arc::new(stapel));
        // M3: die Anzahl wird nur gemeldet, nicht gespeichert — sie steht
        // im Stapel, und dort ist sie die Wahrheit.
        let _ = anzahl;
        Ok(Self {
            bitmap: BitmapFont::builtin(),
            tt: messung.tt_font(),
            quelle: SchriftQuelle::Webfont(name.to_string()),
            // GEMESSEN 05.10.2026: Standard `false` — keine Verzerrung.
            synthetisch_fett: false,
            synthetisch_kursiv: false,
            messung,
            keine: false,
            cache: GlyphCache::new(),
            bitmap_cache: HashMap::new(),
            basen: HashMap::new(),
        })
    }

    /// Wie [`Schrift::mit_systemschrift`], aber mit eigenen Wurzeln.
    ///
    /// Das ist der Weg für Tests: sie brauchen eine Schrift, die
    /// sicher da ist, und keine, die ein System vielleicht nicht hat.
    pub fn aus_wurzeln(wurzeln: &[&Path]) -> Self {
        Self::suche_schnitt(wurzeln, false, false)
    }

    /// Wie [`Schrift::mit_gewicht`], aber mit eigenen Wurzeln.
    ///
    /// GEMESSEN 06.10.2026, warum es diesen Weg geben musste:
    /// [`Schrift::mit_gewicht`] hat die vier Systemwurzeln fest eingetragen,
    /// und auf dieser Maschine liegen an allen vier **auch** die fetten
    /// Schnitte (`FreeSansBold.ttf`). Der Zweig „Wunsch fett, Schnitt fehlt"
    /// — also der Zweig, in dem [`Schrift::synthetisch_fett`] eingeschaltet
    /// wird — war also mit dem oeffentlichen Wunsch-Weg **nicht erreichbar**.
    /// Wer nur `/usr/share/fonts` durchsuchen kann, sieht nie eine
    /// synthetische Schrift.
    ///
    /// GEMESSEN, dass das kein theoretisches Loch ist: `suche_schnitt` nimmt
    /// `find_best_font(..., fett, kursiv)`, das einen Strafpunkt gibt und
    /// sonst das regulaere erste zurueckgibt (narsil-text, stapel.rs:895/898).
    /// Die Suche **findet also immer eine Schrift** — die Frage ist nur, ob
    /// sie die verlangte ist, und genau das sagt das Flag.
    ///
    /// # DAS ZWEITE RUECKGABEWERT, und es ist die wahre Antwort
    ///
    /// `true` heisst: **der Wunsch ist erfuellt** — es steht ein Schnitt da,
    /// der fett/kursiv ist, wie verlangt, und es wurde nichts verzerrt.
    /// `false` heisst: es wurde der naechstbeste Schnitt genommen und, wo
    /// die Achse fehlte, **synthetisch** nachgeholfen (CSS Fonts 4 §6.3:
    /// ein fehlender Schnitt wird kuenstlich erzeugt, bevor ein anderer
    /// Schnitt derselben Familie genommen wird).
    ///
    /// GEMESSEN, warum das nicht „irgendeine Schrift gefunden" heissen darf:
    /// die alte Fassung von `mit_gewicht` gab genau das zurueck
    /// (`!matches!(s.quelle, SchriftQuelle::Bitmap)`), und ein Test mit dem
    /// Wunsch 900 — fuer den es keinen Schnitt gibt — war damit gruen,
    /// obwohl der Wunsch unerfuellt blieb.
    pub fn aus_wurzeln_mit_stil(wurzeln: &[&Path], gewicht: u16, kursiv: bool) -> (Self, bool) {
        let fett = narsil_text::gewicht_ist_fett(gewicht);
        let s = Self::suche_schnitt(wurzeln, fett, kursiv);
        // GEMESSEN: `bitmap()` heisst "nichts gefunden" — das ist der
        // einzige Fall, in dem es keinen Schnitt gibt. Sonst haben wir
        // einen gefunden, auch wenn es nicht genau der verlangte ist, und
        // dann sagt die Verzerrung, ob der Wunsch erfuellt ist.
        let erfuellt = !matches!(s.quelle, SchriftQuelle::Bitmap)
            && !s.synthetisch_fett
            && !s.synthetisch_kursiv;
        (s, erfuellt)
    }

    /// Wie [`Schrift::mit_systemschrift`], aber mit einem anderen Schnitt.
    ///
    /// RUNDE 3 S3: `font-weight` verlangt einen FETTEN Schnitt. Die Schrift
    /// wird genau einmal geladen (crates/narsil/src/main.rs:122) und
    /// behalten, also kann ein Schnittwechsel nicht durch Neuladen
    /// geschehen — er muss die Datei wechseln koennen.
    ///
    /// GEMESSEN 03.10.2026 mit fontTools auf dieser Maschine:
    ///   FreeSans.ttf         usWeightClass = 400
    ///   FreeSansBold.ttf     usWeightClass = 600
    /// Es gibt auf dieser Maschine KEINEN 600er-Schnitt, und bei 600 ist
    /// `FreeSansBold` schon der fette — wer 700 als Schwelle nähme, nähme
    /// den regulaeren Schnitt und der Text wuerde gar nicht fett.
    ///
    /// # WENN ES DEN SCHNITT NICHT GIBT
    ///
    /// GEMESSEN 06.10.2026, KORREKTUR dieses Abschnitts: hier stand
    /// „Es wird KEIN kuenstlich fetter Text gezeichnet". Das war eine
    /// Entscheidung von vor dem 05.10. — und sie machte das synthetische
    /// Fett (AUTONOM.txt:669, [`crate::synthetisch`]) unerreichbar, weil
    /// `suche_schnitt` die Flags auf `false` stehen liess und auf dieser
    /// Maschine an allen vier Wurzeln ein fetter Schnitt **liegt**. Jetzt
    /// gilt CSS Fonts 4 §6.3: der naechstbeste Schnitt wird genommen und
    /// die fehlende Achse **synthetisch** erzeugt
    /// (`Schrift::synthetisch_fett` / `Schrift::synthetisch_kursiv`).
    ///
    /// GEMESSEN, warum das kein stiller Umbruch-Verschieb ist: der
    /// Vorschub kommt aus `hmtx` der gewaehlten Datei und waechst dadurch
    /// **nicht**. Nur die Kontur wird dicker (`glyphe_mischen`), der
    /// Zeichenabstand bleibt der der regulaeren Schrift — wie bei einem
    /// echten fetten Schnitt waechst er nicht, sondern er ist ein anderer
    /// Wert aus `hmtx`. Wer das anders haben will, braucht `Messung`
    /// neu bauen; das steht beim Umbruch.
    ///
    /// Das zweite Rueckgabewert ist jetzt **„der Wunsch ist erfuellt"** und
    /// nicht mehr „irgendeine Schrift gefunden"; siehe
    /// [`Schrift::aus_wurzeln_mit_stil`], das dieselbe Rechnung macht.
    ///
    /// # WAS DER AUFRUFPUNKT TUN MUSS
    ///
    /// Nach dem Wechsel muss der Aufrufer die Schrift neu messen lassen
    /// (`narsil_layout::Messung::systemschrift` mit der neuen Schrift), weil
    /// die gemessene Linie zur alten Schrift gehoert.
    ///
    /// GEMESSEN 06.10.2026, KORREKTUR des Absatzes darunter: hier stand,
    /// eine Methode dafuer gebe es nicht, weil im Zeichenpfad ein
    /// `basis: Option<Point>` gestaeckt habe, das nirgends gesetzt wurde.
    /// GEMESSEN ist jetzt: `grep -rn '\bbasis\b' crates/narsil-raster/src/*.rs`
    /// findet **nur diese Zeile** — das Feld existiert nicht mehr. Der
    /// Ursprung kommt je Aufruf herein: [`Schrift::zeichne`] und
    /// [`Schrift::zeichne_mit_deko`] nehmen `origin: Point`, und die
    /// Grundlinie wird darueber gesetzt (deko.rs: `deko_an_grundlinie`).
    /// Es gibt also nichts nachzutraegen, was diese Schrift nicht selbst
    /// mitbringt — die Zeilenhoehe gehoert zur `Messung`, und die trägt
    /// die neue Schrift bereits.
    pub fn mit_gewicht(gewicht: u16, kursiv: bool) -> (Self, bool) {
        // GEMESSEN: die Wurzeln sind dieselben wie in `mit_systemschrift`.
        let pfade = narsil_text::systemschrift_wurzeln();
        let wurzeln: Vec<&Path> = pfade.iter().map(|p| p.as_path()).collect();
        Self::aus_wurzeln_mit_stil(&wurzeln, gewicht, kursiv)
    }

    /// Baut die gezeichnete **und** gemessene Schrift aus **einer** gewaehlten
    /// Systemschrift: der Stapel, die Kategorie, die `Messung` und die beiden
    /// Verzerrungs-Flaggen.
    ///
    /// 04.10.2026 (SCHRIFT, (c)): dieser Block stand vorher im Rumpf von
    /// `suche_schnitt` und ist fuer die **Familie** (`font-family`) in den
    /// Helfer gewandert, damit beide Wege dieselbe Datei bauen.
    ///
    /// GEMESSEN, was der Block tut (alles an Ort und Stelle belegt):
    /// 1. Die Kategorie kommt **gemessen** aus `schrift_kategorie`, nicht
    ///    geraten — sonst staende Index 0 ohne Kategorie und `glyphe_fuer`
    ///    naehme Index 1 (03.10.2026: Arabisch, 26 leere Spalten).
    /// 2. Der Systemstapel kommt dahinter: Pfeile, CJK und Emoji bekommen so
    ///    eine Glyphe statt eines Kaestchens.
    /// 3. `Messung::mit_stapel`, wenn der Stapel mehr als eine Schrift traegt,
    ///    sonst `Messung::systemschrift`.
    /// 4. `synthetisch_fett`/`synthetisch_kursiv` stehen **hier** (06.10.2026:
    ///    woanders waren sie ein toter Schalter).
    fn schrift_aus_systemfont(
        kandidat: &SystemFont,
        tt: TrueTypeFont,
        fett: bool,
        kursiv: bool,
    ) -> Self {
        let tt = Arc::new(tt);
        let mut stapel = Schriftstapel::neu();
        //
        // DIE KATEGORIE DER HAUPTSCHRIFT, und sie ist der Grund fuer
        // diese Zeile.
        //
        // GEMESSEN 03.10.2026: ohne sie steht die hauptschrift an Index
        // 0 mit `kategorie = None`, weil `von_pfad` sie nicht kennt.
        // `glyphe_fuer` nimmt die Hauptschrift aber ueber
        // `haupt_index()` — die ERSTE mit bekannter Kategorie. GEMESSEN
        // war Index 1, FreeMono, und die Arabisch-Glyphen landeten
        // dort: 26 leere Spalten innerhalb von 121 px Tinte
        // (crates/narsil-raster/tests/arabisch_tinte.rs).
        //
        // Deshalb wird die Kategorie aus der Systemsuche geholt statt
        // geraten. GEMESSEN: die Abdeckungsrunde in
        // `ersatzschriften_waehlen` laedt inzwischen DejaVuSans in den
        // Systemstapel, und die hat die Praesentationsformen.
        //
        // `unwrap_or` an EINER Stelle und nicht an beiden: die Datei
        // kann auch ohne Kategorie in den Stapel, aber der Systemstapel
        // braucht eine. Ohne sie waere er nicht baubar — und
        // `SansSerif` ist hier begruendbar statt geraten: GEMESSEN
        // fällt `find_system_fonts` bei unbekanntem Namen ebenfalls auf
        // SansSerif.
        let kategorie = schrift_kategorie(&kandidat.path, fett, kursiv);
        let system_kat = kategorie.unwrap_or(FontCategory::SansSerif);
        if let Some(haupt) =
            kategorie.and_then(|k| Stapelschrift::von_systemfont_pfad("haupt", &kandidat.path, k))
        {
            let _ = stapel.hinzufuegen(haupt);
        }
        if let Ok(system) = system_stapel(system_kat, fett, kursiv) {
            for i in 0..system.len() {
                if let Some(sch) = system.schrift(i) {
                    let _ = stapel.hinzufuegen(sch.clone());
                }
            }
        }
        let messung = if stapel.len() > 1 {
            narsil_layout::Messung::mit_stapel(tt, Arc::new(stapel))
        } else {
            narsil_layout::Messung::systemschrift(tt)
        };
        Self {
            bitmap: BitmapFont::builtin(),
            tt: messung.tt_font(),
            quelle: SchriftQuelle::System(pfad_als_text(&kandidat.path)),
            //
            // DIE VERZERRUNG, und sie wird HIER gesetzt — das ist der
            // ganze Unterschied zu `Stilauswahl` (narsil-text, stapel.rs:
            // 915/916), das dieselbe Rechnung macht:
            //
            //     synthetisch_fett:   fett   && !gewaehlt.bold
            //     synthetisch_kursiv: kursiv && !gewaehlt.italic
            //
            // GEMESSEN 06.10.2026: an dieser Stelle stand `false`, und
            // damit war das synthetische Fett (AUTONOM.txt:669) nur ueber
            // Beispiel und Test erreichbar — `grep -rn 'synthetisch_fett =
            // true' crates/` findet ausserhalb von `tests/` und
            // `examples/` keine Stelle. Die Felder waren gesetzt und
            // wurden gelesen (`glyphe_mischen`), aber NIE eingeschaltet:
            // ein toter Schalter, diesmal in meinem eigenen Crate.
            //
            // `kandidat` ist die Datei, die die Suche wirklich gewaehlt
            // hat, und `SystemFont.bold`/`.italic` stehen GEMESSEN am
            // Dateinamen (system.rs:152/157, dort der Grund). Also stimmt
            // die Rechnung mit der Schnitt-Suche ueberein: die `bekannt`
            // und `find_best_font` suchen nach denselben Woertern.
            //
            // GEMESSEN, dass der Normalfall unveraendert bleibt: bei
            // `fett = false, kursiv = false` ist das Ergebnis `false &&
            // …` = `false`. `Schrift::mit_systemschrift()` (der Weg, den
            // narsil nimmt) ruft ueber `aus_wurzeln` genau diesen Zweig.
            synthetisch_fett: fett && !kandidat.bold,
            synthetisch_kursiv: kursiv && !kandidat.italic,
            messung,
            keine: false,
            cache: GlyphCache::new(),
            bitmap_cache: HashMap::new(),
            basen: HashMap::new(),
        }
    }

    /// Die Schrift einer **Familienliste** — der Weg, den eine Seite mit
    /// `font-family` nimmt (04.10.2026, SCHRIFT (c)).
    ///
    /// # WAS GEMESSEN IST
    ///
    /// GEMESSEN 04.10.2026, bevor diese Funktion da war: `DisplayItem::Text`
    /// fuehrte `gewicht` und `kursiv`, aber **keine** Familie. Jeder Text
    /// ging damit ueber `suche_schnitt`, und der nennt an erster Stelle
    /// `BEKANNT_SCHRIFTEN[0]` — GEMESSEN `DejaVuSans.ttf`, **egal was die
    /// Seite geschrieben hatte**. `<code>`, `<kbd>` und `<samp>` standen
    /// so GEMESSEN in `DejaVuSans.ttf` statt in der Festbreitenschrift,
    /// obwohl das UA-Blatt fuer sie `monospace` sagt.
    ///
    /// GEMESSEN, dass `familie_waehlen` die richtige Datei nennt: der Test
    /// `crates/narsil-text/tests/a54_familien_orakel.rs` vergleicht gegen
    /// `fc-match` und ist GEMESSEN **3/3** gruen fuer `serif`,
    /// `sans-serif`, `monospace` — dieselbe Datei, die der Browser nimmt.
    /// (Fuenf weitere Familien auch; die vier mit CFF/OTF-Dateien nicht,
    /// weil `TrueTypeFont` keine CFF lesen kann — dort wird der lesbare
    /// Nachbar genommen.)
    ///
    /// # DER AUFFANG
    ///
    /// Eine **leere** Liste, eine unbekannte Familie und eine nicht
    /// lesbare Datei fuehren alle zu `mit_gewicht` — also zum
    /// Fliesstextschnitt. CSS Fonts 4 §4.2 behandelt eine fehlende Familie
    /// genau so, und eine leere Seite ist schlimmer als Kaestchen.
    pub fn mit_familie(familie: &str, gewicht: u16, kursiv: bool) -> (Self, bool) {
        let liste = familie.trim();
        if liste.is_empty() {
            return Self::mit_gewicht(gewicht, kursiv);
        }
        // Release 09.10.2026: angemeldete Webschriften (`@font-face`) zuerst
        // — dieselbe Auswahl wie die Messung (`stapel_fuer_familie`).
        if let Some(t) = narsil_text::webschrift::finden(liste, gewicht, kursiv) {
            return (Self::aus_webtreffer(t), true);
        }
        let pfade = narsil_text::systemschrift_wurzeln();
        let wurzeln: Vec<&Path> = pfade.iter().map(|p| p.as_path()).collect();
        match Self::suche_familie(&wurzeln, liste, gewicht, kursiv) {
            Some((kandidat, tt)) => {
                let fett = narsil_text::gewicht_ist_fett(gewicht);
                // GEMESSEN, warum die Verzerrung hier genauso gesetzt wird
                // wie in `schrift_aus_systemfont`: es ist dieselbe Rechnung
                // ueber dieselben Felder (`SystemFont::bold`/`italic`), und
                // ohne sie waere ein fetter Wunsch still aufrecht
                // (CSS Fonts 4 §6.3).
                let s = Self::schrift_aus_systemfont(&kandidat, tt, fett, kursiv);
                let erfuellt = !matches!(s.quelle, SchriftQuelle::Bitmap);
                (s, erfuellt)
            }
            None => Self::mit_gewicht(gewicht, kursiv),
        }
    }

    /// Die **eine** Stelle, die aus einer Familienliste eine Datei macht.
    ///
    /// GEMESSEN, warum sie ueber `familie_waehlen` (narsil-text) geht und
    /// nicht ueber `BEKANNT_SCHRIFTEN`: `BEKANNT_SCHRIFTEN` ist eine
    /// **Reihenfolge fuer den Fliesstext** — sie sagt, welche Datei man
    /// nimmt, wenn die Seite nichts nennt. Hier hat die Seite etwas
    /// genannt, und die Liste in CSS Fonts 4 §4.2 sagt, dass der erste
    /// Eintrag gewinnt, fuer den es eine Schrift gibt.
    ///
    /// GEMESSEN, warum Buchstaben **und** Umrisse geprueft werden, aber
    /// feste Breite **erlaubt** ist: `suche_schnitt` schliesst Monospace
    /// aus (dort waere es der Fliesstext), und hier ist `monospace`
    /// genau der Fall, fuer den die Seite geschrieben hat. GEMESSEN sind
    /// **37** von 109 Systemschriften ohne `glyf`/`loca` (36 CFF plus
    /// `NotoColorEmoji.ttf`) — die duerfen nicht die Hauptschrift werden.
    fn suche_familie(
        wurzeln: &[&Path],
        liste: &str,
        gewicht: u16,
        kursiv: bool,
    ) -> Option<(SystemFont, TrueTypeFont)> {
        let _ = wurzeln;
        // DIE EINE AUSWAHL (04.10.2026, SCHRIFT_9): `datei_fuer_familie`
        // ist dieselbe Funktion, die die **Messung** benutzt
        // (`narsil_text::stapel_fuer_familie` -> `datei_fuer_familie`).
        //
        // GEMESSEN, warum das noetig war: vorher suchte der Raster ueber
        // `familie_waehlen` selbst und die Messung gar nicht nach der
        // Familie — das war der Fehler aus SCHRIFT_9 (Umbruch misst mit
        // Sans, Raster zeichnet in der Familie). Zwei Auswahlwege sind
        // zwei Moeglichkeiten fuer „dieselbe Datei"; eine ist gemessen
        // sicher.
        //
        // GEMESSEN, warum die **Wurzeln** hier nicht mehr gebraucht
        // werden: `datei_fuer_familie` liest dieselben vier Wurzeln
        // (`narsil-text/src/familie.rs`, `WURZELN`) und haelt sie
        // thread-local. Das Parameter-Argument bleibt, weil der
        // oeffentliche Weg (`mit_familie`) es weiterhin uebergibt.
        let pfad = narsil_text::datei_fuer_familie_mit_gewicht(liste, gewicht, kursiv)?;
        let kandidat = {
            let schriften = narsil_text::systemschriften();
            schriften.iter().find(|f| f.path == pfad)?.clone()
        };
        // Eine kaputte Datei ist kein Grund aufzugeben: es gab hier
        // keine naechste Wurzel mehr, der Aufrufer nimmt dann
        // `mit_gewicht` (der Auffang, CSS Fonts 4 §4.2).
        let tt = load_system_font(&kandidat.path).ok()?;
        Some((kandidat, tt))
    }

    /// Die Suche, mit Schnittwaehlen. Die oeffentlichen Wege sind Duenste
    /// davon, damit es nur eine Stelle gibt, die eine Schriftdatei oeffnet.
    fn suche_schnitt(wurzeln: &[&Path], fett: bool, kursiv: bool) -> Self {
        // M3: die Zahl der Wurzeln ist Eingabe, also begrenzt.
        // Release 08.10.2026: die Systemwurzeln kommen aus dem EINEN Scan
        // des Prozesses (`narsil_text::systemschriften`). Vorher las jeder
        // Schnitt (normal, fett, kursiv, fett-kursiv) und jeder Faden das
        // ganze Schriftverzeichnis neu — unter Windows mehrere hundert
        // Dateien, jede vollstaendig, je Aufruf.
        let system_wurzeln = narsil_text::systemschrift_wurzeln();
        for wurzel in wurzeln.iter().take(MAX_SUCHWURZELN) {
            // `find_system_fonts` ist in `narsil-text` begrenzt (Tiefe 5,
            // 1000 Dateien). Ein Fehler heisst "hier ist nichts", nicht
            // "das ganze System ist kaputt" — darum wird weitergegangen.
            let schriften: Vec<narsil_text::SystemFont> = if system_wurzeln.iter().any(|w| w.as_path() == *wurzel) {
                narsil_text::systemschriften()
                    .iter()
                    .filter(|f| f.path.starts_with(wurzel))
                    .cloned()
                    .collect()
            } else {
                let Ok(s) = find_system_fonts(wurzel) else {
                    continue;
                };
                s
            };
            //
            // ZUERST DIE BEKANNTEN FAMILIEN, und das ist keine
            // Geschmacksfrage. GEMESSEN am 30.09.2026, 09:30: ohne diese
            // Liste nahm `find_best_font` die ERSTE sans-serif-Schrift
            // im Verzeichnis, und das war `Symbola_hint.ttf` aus
            // `truetype/ancient-scripts` — ein Paket fuer alte
            // Schriftsysteme. Die Seite war lesbar, aber jeder Buchstabe
            // sah aus, als kaeme er aus einem Museum.
            //
            // Und gegen den DATEINAMEN, nicht gegen `family`:
            // `find_system_fonts` liefert fuer `FreeSans.ttf` die Familie
            // `"free"` (gemessen 09:35), ein Vergleich gegen "freesans"
            // fand also NICHTS.
            //
            // **DER RANG STATT DER REIHENFOLGE**, gemessen 03.10.2026
            // 20:4x (SCHRIFT). `.find()` nimmt den **ersten** Treffer in der
            // Lesereihenfolge von `find_system_fonts`, nicht den besten aus
            // `BEKANNT_SCHRIFTEN`. Auf dieser Maschine ist `/usr/share/fonts/
            // truetype/freefont` vor `…/truetype/dejavu` dran, also gewann
            // `FreeSans.ttf` — und `BEKANNT_SCHRIFTEN` sagt an erster Stelle
            // `"dejavusans"`.
            //
            // GEMESSEN, was das kostet (`texttest.html`, Text `AvWaTo
            // HHxbxb`, 16 px, Tinte in einem Bild gemessen):
            //
            // ```text
            //    Firefox (fc-match sans-serif -> DejaVuSans.ttf)   127 px
            //    narsil    (FreeSans.ttf)                              115 px   0.906
            //    Huelle aus DejaVuSans selbst (a45_tinte)            133 px
            // ```
            //
            // GEMESSEN, dass es die **Schrift** ist und nicht der Rasterizer
            // (zwei Beispiele): `a45_tinte` misst `Tinte/Rasterbreite =
            // 1,000` — es geht kein Rand verloren; und `a46_stapel_breite`
            // zeigt den Vorschub je Zeichen, Stack gegen die verlangte
            // Schrift: Leerzeichen 4,000 gegen 5,086 px, `x` 7,616 gegen
            // 9,469 px. Das sind FreeSans-Vorschuebe.
            //
            // GEMESSEN, wohin Firefox schaut: `fc-match sans-serif` →
            // `DejaVuSans.ttf: "DejaVu Sans" "Book"`. Der Browser nimmt also
            // **dieselbe** Datei, die `BEKANNT_SCHRIFTEN` an erster Stelle
            // nennt — die Liste war richtig, nur wurde sie nicht beachtet.
            //
            // Die Pruefung selbst bleibt unveraendert: nur die **Wahl unter
            // den passenden** Kandidaten ist neu, und sie ist genau die
            // Reihenfolge der Liste.
            let bekannt = schriften
                .iter()
                .filter_map(|f| {
                    let n = f
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())?
                        .to_ascii_lowercase();
                    // GEMESSEN 09:40: mit `starts_with` allein gewann
                    // `FreeSansOblique.ttf`, weil es vor `FreeSans.ttf`
                    // im Verzeichnis steht. Fuer den Fliesstext gehoert
                    // der NORMALE Schnitt, nicht die kursive.
                    //
                    // RUNDE 3 S3: fuer einen FETTEN Wunsch wird
                    // umgekehrt nach dem fetten Schnitt gesucht, und
                    // die KURSE wird ausgeschlossen. `STIL_ENDS` ist die
                    // Liste der Stilschnitte im Dateinamen; ein fetter
                    // Schnitt, der nicht "bold" heisst (etwa "Black"),
                    // wird dabei nicht gefunden — GEMESSEN heisst auf
                    // dieser Maschine jeder fette Schnitt "bold".
                    //
                    // GEMESSEN 03.10.2026 an den vier Dateien im
                    // Verzeichnis, mit der Regel von unten:
                    //   freesans.ttf              regulaer: ja
                    //   freesansbold.ttf          fett:     ja
                    //   freesansoblique.ttf       regulaer: NEIN
                    //                                        fett:     NEIN
                    //   freesansboldoblique.ttf   fett:     ja
                    //                                        regulaer: NEIN
                    //
                    // `freesansoblique.ttf` fiel in BEIDE Regeln und
                    // wurde darum von keiner gefunden. Die erste Fassung
                    // dieser Suche kannte nur "fett oder nicht", und ein
                    // Test, der nur pruefte "keine Bitmap", war gruen —
                    // `FreeSans.ttf` ist keine Bitmap.
                    //
                    // Die Regel ist darum: gewaehlt wird die Datei, deren
                    // Stilschnitte GENAU den Wunsch erfuellen. Beide
                    // Richtungen werden geprueft, und `italic` wie
                    // `oblique` zaehlen als kursiv (CSS Fonts 4 §6.3).
                    let hat_italic = n.contains("italic") || n.contains("oblique");
                    let hat_fett = n.contains("bold");
                    let passt_zum_wunsch = match (fett, kursiv) {
                        (false, false) => !hat_fett && !hat_italic,
                        (true, false) => hat_fett && !hat_italic,
                        (false, true) => !hat_fett && hat_italic,
                        (true, true) => hat_fett && hat_italic,
                    };
                    //
                    // UND: keine ANDEREN Stilschnitte.
                    //
                    // GEMESSEN 03.10.2026: `STIL_ENDS` enthaelt neben
                    // `bold`, `italic` und `oblique` auch `condensed`,
                    // `narrow` und `light`. Ohne diese zweite Bedingung
                    // wuerde `FreeSansNarrow.ttf` den regulaeren
                    // Schnitt gewinnen — es ist weder fett noch kursiv
                    // und waere nach der Tabelle oben passend.
                    //
                    // Die Tabelle kann das nicht sehen, weil auf dieser
                    // Maschine keine Narrow-Datei liegt (GEMESSEN: nur
                    // die vier FreeSans-Dateien). Die Bedingung ist
                    // trotzdem noetig, sonst waere sie ein Fehler auf
                    // der naechsten Maschine.
                    let anderer_stilschnitt =
                        STIL_ENDS.iter().any(|st| n.contains(st)) && !hat_fett && !hat_italic;
                    //
                    // DIE FESTE BREITE, und das ist die zweite
                    // Korrektur vom 03.10.2026.
                    //
                    // GEMESSEN, warum `starts_with` hier gefaehrlich ist:
                    // `"notosansmono-regular".starts_with("notosans")` ist
                    // `true` — `BEKANNT_SCHRIFTEN` enthaelt `"notosans"`, und
                    // `NotoSansMono-Regular.ttf` ist eine **Monospace**-
                    // Schrift. Sie fiel deshalb durch alle Filter:
                    // `STIL_ENDS` (schrift.rs) kennt `bold`, `italic`,
                    // `oblique`, `condensed`, `narrow` und `light` — aber
                    // **kein** `"mono"`.
                    //
                    // GEMESSEN, wann es sichtbar wird: `/usr/share/fonts`
                    // enthaelt GEMESSEN **drei** Dateien in
                    // `truetype/noto` (NotoColorEmoji, NotoSansMono-Bold,
                    // NotoSansMono-Regular). Steht `truetype/noto` in der
                    // Wurzelliste **vor** `truetype/freefont`, gewinnt
                    // `NotoSansMono-Regular.ttf`, und die ganze Seite ist
                    // feste Breite:
                    //
                    // GEMESSEN: [noto, freefont] -> System(".../NotoSansMono-Regular.ttf")
                    // GEMESSEN: [freefont, noto] -> System(".../FreeSans.ttf")
                    //
                    // GEMESSEN als Orakel (fontTools, Breite in em):
                    //
                    //   NotoSansMono-Regular  i 0,600 M 0,600 W 0,600 -> 1 verschiedene
                    //   FreeSans              i 0,222 M 0,846 W 0,937 -> 6 verschiedene
                    //
                    // GEMESSEN, warum die KATEGORIE und nicht der Name:
                    // `classify_font` (narsil-text system.rs) liest den
                    // Dateinamen und erkennt GEMESSEN `mono` — die Datei
                    // traegt also `FontCategory::Monospace`, und das ist
                    // eine Eigenschaft der Schrift statt eine Eigenschaft
                    // ihres Namens.
                    //
                    // GEMESSEN, warum das `suche_schnitt` und nicht
                    // `waehle_schrift` verbietet: `suche_schrift` kennt
                    // die **Familie** aus dem CSS und darf Monospace
                    // liefern, wenn `font-family: monospace` verlangt ist.
                    // `suche_schnitt` baut den Schnitt, den **kein** Wunsch
                    // benannt hat — den Fliesstext. Der ist nie Monospace
                    // (CSS Fonts 4 §6.2).
                    let keine_feste_breite = f.category != FontCategory::Monospace;
                    if !(passt_zum_wunsch && !anderer_stilschnitt && keine_feste_breite) {
                        return None;
                    }
                    // Die Stelle in `BEKANNT_SCHRIFTEN`: 0 ist der
                    // Wunsch, 7 die letzte Hoffnung. `None` heisst
                    // "steht nicht in der Liste" — das ist derselbe
                    // Fall wie vorher (`any(...) == false`).
                    BEKANNT_SCHRIFTEN
                        .iter()
                        .position(|b| n.starts_with(b))
                        .map(|rang| (rang, f))
                })
                .min_by_key(|(rang, f)| {
                    // **(GEMESSEN 03.10.2026 20:5x)** zwei Kandidaten
                    // koennen denselben Rang haben: `DejaVuSans-Bold.ttf`
                    // und `DejaVuSansCondensed-Bold.ttf` beginnen beide
                    // mit `"dejavusans"`, und der Stilschnitt-Filter
                    // laesst beide zu, sobald der Wunsch **fett** ist
                    // (`anderer_stilschnitt` gilt nur fuer den
                    // regulaeren Wunsch). GEMESSEN, dass ohne die
                    // zweite Zahl `DejaVuSansCondensed-Bold.ttf` gewinnt
                    // — in der Lesereihenfolge steht es zuerst — und ein
                    // **verengtes** Gesicht fuer den Fliesstext ist
                    // genau der Fehler, den die Breite jetzt zeigt.
                    //
                    // Der kuerzere Dateiname ist der einfachere
                    // Schnitt: GEMESSEN auf dieser Maschine
                    //   dejavusans.ttf (14) < dejavusans-bold.ttf (20)
                    //   < dejavusanscondensed-bold.ttf (27)
                    // Die Zahl ist also kein Rateversuch, sie ist die
                    // Laenge des Namens — und sie macht die Wahl
                    // unabhaengig von der Lesereihenfolge.
                    let laenge = f.path.file_name().map_or(usize::MAX, |n| n.len());
                    (*rang, laenge)
                })
                .map(|(_, f)| f);
            // Ohne Serifen zuerst: eine Seite wird GELESEN. Und eine
            // Serifenschrift hat an den meisten Stellen Strichstaerken,
            // die bei 16 px als Text lesbar sind und als Ueberschrift zu
            // schwer wirken.
            // Der Rueckfall auf die KATEGORIE traegt den Schnitt mit:
            // `find_best_font` bewertet `bold`/`italic` mit +2 und +4
            // Punkten, es verschiebt also einen regulaeren Schnitt nur,
            // wenn kein passender da ist.
            let sans = find_best_font(&schriften, FontCategory::SansSerif, fett, kursiv);
            let mono = find_best_font(&schriften, FontCategory::Monospace, fett, kursiv);
            //
            // DIE BUCHSTABEN, und das ist die Korrektur vom 03.10.2026.
            //
            // GEMESSEN, warum hier gefiltert werden muss: `find_best_font`
            // (narsil-text system.rs) gibt **nie leer** zurueck — GEMESSEN sind
            // drei Stufen, die letzte ist `fonts.first()`. Es lieferte also
            // auch dann eine Schrift, wenn die nur Ergaenzungszeichen hat.
            //
            // GEMESSEN mit fontTools ueber alle 109 Schriftdateien unter
            // `/usr/share/fonts`: **40** sind als Fliesstext unbrauchbar, und
            // sie zerfallen in zwei Gruppen:
            //
            // * **4** echte Ergaenzungsschriften ohne `M` — darunter
            //   `noto/NotoColorEmoji.ttf`.
            // * **36** CFF-Dateien (`.otf`), die `TrueTypeFont::from_bytes`
            //   gar nicht laedt (truetype.rs:218 akzeptiert nur `0x00010000`
            //   und `true`, eine CFF-Datei traegt `OTTO`).
            //
            // `NotoColorEmoji.ttf` ist der Fall, der den Test ausloest: es
            // liegt in `/usr/share/fonts/truetype/noto`, `classify_font`
            // stuft es als **SansSerif** ein (der Name enthaelt weder „mono"
            // noch „serif"), und GEMESSEN hat das Verzeichnis nur drei
            // Dateien. Ohne diesen Filter lieferte `suche_schnitt` aus
            // genau diesem Verzeichnis eine Schrift, in der **kein „M"
            // existiert** — jedes Wort ein Kaestchen.
            //
            // M3: `hat_buchstaben` ist ein Feld, der Filter kostet keine
            // Allokation und laeuft ueber dieselbe Liste, die
            // `find_system_fonts` (MAX_FONTS = 1000) begrenzt hat.
            //
            // DIE DREI EIGENSCHAFTEN EINES FLIESSTEXT-SCHNITTES, und alle
            // drei gehoeren in **einen** Filter:
            //
            // 1. er kann Buchstaben (`hat_buchstaben`),
            // 2. er hat Umrisse (`hat_umrisse`), und
            // 3. er hat keine feste Breite (`category != Monospace`).
            //
            // GEMESSEN zu 2, am 03.10.2026 (A27): nach dem Zulassen von
            // `OTTO` in `TrueTypeFont::from_bytes` (truetype.rs:218) sind
            // GEMESSEN **37** von 109 Systemschriften ohne `glyf`/`loca`:
            // 36 CFF-Dateien plus `NotoColorEmoji.ttf` (CBDT-Bitmaps). Eine
            // davon hat `M = Some(46)` — also Buchstaben — und wuerde als
            // Haupt-Schrift einen richtigen Vorschub und eine **leere Seite**
            // liefern.
            //
            // GEMESSEN, warum das zwei sein muessen: der erste Filter allein
            // liess GEMESSEN `NotoSansMono-Bold.ttf` als regulaeren Schnitt
            // stehen — die Wurzel `truetype/noto` hat GEMESSEN nur drei
            // Dateien, und nach dem Buchstaben-Filter bleiben genau die zwei
            // Monospace-Schnitte uebrig. Die Suche beendete sich in dieser
            // Wurzel, obwohl `truetype/freefont` eine echte Leseschrift hat.
            //
            // GEMESSEN, warum die Suche hier **weitergeht** und nicht nimmt:
            // das `let Some(kandidat) = ... else { continue }` unter dieser
            // Zeile geht zur naechsten Wurzel, wenn die Kette nichts
            // liefert. Genau das ist der Weg, den der zweite Filter oeffnet.
            //
            // GEMESSEN, warum `find_best_font` trotzdem in der Kette bleibt:
            // es bewertet `fett`/`italic` (schrift.rs Kommentar dort), und
            // ohne es kaeme fuer einen fetten Wunsch der regulaere Schnitt.
            let fliesstext_taugt = |f: &&SystemFont| {
                f.hat_buchstaben && f.hat_umrisse && f.category != FontCategory::Monospace
            };
            //
            // DER REST, und er ist **offen** — nicht vergessen, sondern
            // gemessen.
            //
            // GEMESSEN am 03.10.2026: der Filter hier wirkt innerhalb einer
            // Wurzel, aber die Schleife endet in der **ersten** Wurzel, in der
            // `or_else` etwas geliefert hat. Und `or_else(|| schriften
            // .first())` liefert dort immer etwas. Also:
            //
            // ```text
            // GEMESSEN: nur noto          -> NotoSansMono-Bold.ttf
            // GEMESSEN: [noto, freefont]  -> NotoSansMono-Bold.ttf
            // GEMESSEN: [freefont, noto]  -> FreeSans.ttf
            // ```
            //
            // Die Wurzel `truetype/noto` hat GEMESSEN vier Dateien, und
            // `find_system_fonts` liefert sie in Lesereihenfolge:
            // `NotoSansMono-Bold`, `NotoSansMono-Regular`, `NotoColorEmoji`,
            // `NotoMono-Regular`. `NotoSansMono-Bold` ist die erste — also
            // greift der Notfall, bevor die zweite Wurzel besucht wird.
            //
            // Die Korrektur ist klein, aber sie beruehrt die **Struktur**:
            // `schriften.first()` muss der Fallback **nach** der Schleife
            // sein, nicht in ihr. Dafuer braucht es eine Merkvariable
            // (`let mut notfall: Option<&Path>`) **vor** dem `for` und ein
            // `continue` statt des `or_else`. GEMESSEN, warum das nicht
            // nebenbei ging: der Bau-Block (narsil-raster/schrift.rs, ab
            // `// EINE \`Messung\` fuer beides`) muesste dann **nach** der
            // Schleife noch einmal aufrufbar sein, also aus der Schleife
            // heraus. Das war am 03.10.2026 in **sechs** Versuchen rot, weil
            // `fmt` die Zeilennummern zwischen den Schritten verschiebt.
            //
            // Bis dahin ist die Behauptung, die wirklich gilt: **innerhalb**
            // einer Wurzel wird keine Ergänzungsschrift und keine feste Breite
            // als Fliesstext gewaehlt. Das behauptet
            // `tests/fliess_breite.rs::innerhalb_einer_wurzel_kein_monospace`.
            //
            // Und: eine leere Seite ist schlimmer als Kaestchen — CSS Fonts 4
            // §6.2 behandelt eine fehlende Familie wie `sans-serif`, nicht
            // wie „nichts".
            let kandidat = bekannt
                .or(sans)
                .or(mono)
                .filter(fliesstext_taugt)
                .or_else(|| schriften.iter().find(fliesstext_taugt))
                .or_else(|| schriften.first())
                .cloned();
            let Some(kandidat) = kandidat else {
                continue;
            };
            let Ok(tt) = load_system_font(&kandidat.path) else {
                // Eine kaputte Schriftdatei ist kein Grund, die Suche zu
                // beenden: die nächste Wurzel hat vielleicht eine gute.
                continue;
            };
            // EINE `Messung` fuer beides, und sie traegt die Schrift:
            // der Umbruch misst mit ihr, und der Vorschub beim Zeichnen
            // kommt aus ihr. Zwei Kopien der Schrift waeren ein
            // Kopieren der Tabellen (W5G: die Kopplung ist die Messung).
            // SCHRIFTSTAPEL (Koordination 30.09.): Hauptschrift zuerst, dann die
            // Ersatzschriften des Systems - damit Pfeile, CJK und Emoji eine
            // Glyphe bekommen statt eines Kaestchens. Messung und Zeichnen
            // benutzen DENSELBEN Stapel.
            return Self::schrift_aus_systemfont(&kandidat, tt, fett, kursiv);
        }
        // Nichts gefunden, nichts Lesbares: die Bitmap. Kein Fehler.
        Self::bitmap()
    }

    /// Baut eine Schrift aus den Bytes einer Webfont (`@font-face`).
    ///
    /// # WAS GEMESSEN IST
    ///
    /// GEMESSEN 04.10.2026, S8: die Webfonts, die eine Seite wirklich
    /// anbietet, sind WOFF2. Auf dieser Maschine liegen 18 echte, und **alle
    /// 18 benutzen den glyf-Transform** — der Container wird gelesen
    /// (`narsil_text::woff2`), der Transform in `glyf_transform_zu_glyf_mit_laengen`.
    ///
    /// GEMESSEN, dass das ohne diesen Aufruf nichts waere: `suche_schnitt`
    /// laedt ausschliesslich Dateien aus `/usr/share/fonts`. Eine Webfont aus
    /// dem Netz hat dort keinen Pfad — sie ist ein `Vec<u8>`. Ohne diese
    /// Funktion koennte kein Aufrufer sie zeichnen.
    ///
    /// # DER STAPEL, und das ist der zweite Teil von S8
    ///
    /// Der Webfont kommt **zuerst** in den Stapel, der Systemstapel dahinter.
    /// Das ist der Schriftfallback: ein Zeichen, das der Webfont nicht hat,
    /// wird aus der Systemschrift geholt, und ein Emoji oder ein CJK-Zeichen,
    /// das keine der beiden hat, bleibt ein leerer Kasten — ohne Absturz.
    ///
    /// GEMESSEN, warum die Reihenfolge so ist: `glyphe_fuer` nimmt die erste
    /// passende Schrift im Stapel. GEMESSEN an `test.woff2`: `Roboto` deckt
    /// lateinisch ab, aber kein Emoji, und `Symbola` deckt 4 von 4 der
    /// geprueften Emoji.
    ///
    /// # FEHLER
    ///
    /// `Err` mit einem benannten Text, wenn die Bytes keine Schrift sind. Es
    /// wird KEIN Ersatz gebaut: wer eine kaputte Webfont hat, soll das wissen,
    /// und ein stiller Bitmap-Fallback wuerde die Luecke verstecken.
    ///
    /// # GEMESSENE KETTE
    ///
    /// `unitsPerEm 2048`, `GID von 'A' = 37`, `Vorschub 1336` fuer
    /// `test.woff2` — behauptet in `crates/narsil-text/tests/woff2.rs`.
    ///
    /// # DER NAME, und warum er kein `&'static str` ist (M4)
    ///
    /// GEMESSEN 06.10.2026: das Argument war vorher `&'static str`. Ein
    /// Browser hat den Namen aber aus dem CSS (`String`, aus
    /// `narsil_css::SchriftQuelle::familie`) — der einzige Weg, ein
    /// `&'static str` daraus zu machen, ist `Box::leak`, und das waechst
    /// mit jeder `@font-face`-Regel der Seite **fuer immer**. Der Name wird
    /// jetzt kopiert und gehoert der Schrift.
    /// Release 09.10.2026: eine Schrift aus einem Treffer des
    /// Webschrift-Registers (Webschrift + Systemstapel dahinter).
    pub fn aus_webtreffer(t: narsil_text::webschrift::Treffer) -> Self {
        let tt = Arc::new(t.schrift.font.clone());
        let messung = narsil_layout::Messung::mit_stapel(tt, t.stapel.clone());
        Self {
            bitmap: BitmapFont::builtin(),
            tt: messung.tt_font(),
            quelle: SchriftQuelle::Webfont(t.name.clone()),
            synthetisch_fett: t.fett_fehlt,
            synthetisch_kursiv: t.kursiv_fehlt,
            messung,
            keine: false,
            cache: GlyphCache::new(),
            bitmap_cache: HashMap::new(),
            basen: HashMap::new(),
        }
    }

    pub fn aus_webfont(name: &str, daten: &[u8]) -> Result<Self, String> {
        let haupt = Stapelschrift::von_bytes(name, daten).ok_or_else(|| {
            // GEMESSEN: 12 Byte sind das Minimum fuer einen sfnt-Kopf,
            // 48 fuer eine WOFF2. Alles darunter kann keine Schrift sein.
            if daten.len() < 12 {
                format!(
                    "Webfont {name}: nur {} Byte, das ist kein Schriftkopf",
                    daten.len()
                )
            } else if daten.starts_with(b"wOF2") {
                format!("Webfont {name}: WOFF2 nicht lesbar (Brotli oder glyf-Transform)")
            } else {
                format!("Webfont {name}: kein sfnt und keine WOFF2")
            }
        })?;
        let tt = Arc::new(haupt.font.clone());
        let mut stapel = Schriftstapel::neu();
        // Der Webfont ZUERST — das ist der Fallback in die andere Richtung.
        let _ = stapel.hinzufuegen(haupt);
        // Und der Systemstapel dahinter, damit Emoji und CJK etwas finden.
        //
        // GEMESSEN 04.10.2026: `system_stapel` nimmt die Kategorie aus dem
        // Argument; fuer eine Webfont ohne Angabe ist `SansSerif` die
        // Voreinstellung — dieselbe, die `suche_schnitt` bei unbekanntem Namen
        // nimmt (GEMESSEN dort in PROBLEME.txt ~02:1x).
        if let Ok(sys) = system_stapel(FontCategory::SansSerif, false, false) {
            for i in 0..sys.len() {
                if let Some(s) = sys.schrift(i) {
                    let _ = stapel.hinzufuegen(s.clone());
                }
            }
        }
        let messung = if stapel.len() > 1 {
            narsil_layout::Messung::mit_stapel(tt.clone(), Arc::new(stapel))
        } else {
            narsil_layout::Messung::mit_stapel(tt.clone(), Arc::new(Schriftstapel::neu()))
        };
        Ok(Self {
            bitmap: BitmapFont::builtin(),
            tt: messung.tt_font(),
            quelle: SchriftQuelle::Webfont(name.to_string()),
            // GEMESSEN 05.10.2026: Standard `false` — keine Verzerrung.
            synthetisch_fett: false,
            synthetisch_kursiv: false,
            messung,
            keine: false,
            cache: GlyphCache::new(),
            bitmap_cache: HashMap::new(),
            basen: HashMap::new(),
        })
    }

    /// Der Schriftstapel, den [`Schrift::zeichne`] benutzt.
    ///
    /// GEMESSEN 03.10.2026: das ist kein Rechenweg, sondern die
    /// Sichtbarmachung. `zeichne_truetype` holt jede Glyphe ueber
    /// `self.messung.stapel()`, und welches Zeichen in welcher Schrift
    /// landet, war von aussen nicht sichtbar — dabei sind genau diese
    /// Fragen die Ursache der arabischen Luecken gewesen
    /// (team/log/befund_arabisch_1200.png). Ohne diesen Getter musste man
    /// die Kette erraten.
    #[must_use]
    pub fn messung_stapel(&self) -> Option<Arc<Schriftstapel>> {
        self.messung.stapel()
    }

    /// Woher die Schrift kommt.
    ///
    /// GEMESSEN 06.10.2026: hier stand `self.quelle`, und das ging nur,
    /// weil `SchriftQuelle` `Copy` war. Seit die Namen eigene `String`
    /// sind (siehe das `enum`), ist es ein Klon — und der kostet fuer
    /// `Bitmap` nichts, fuer einen Pfad die Laenge des Pfades.
    pub fn quelle(&self) -> SchriftQuelle {
        self.quelle.clone()
    }

    /// Zeichnet einen Text an eine Stelle.
    ///
    /// Rückgabe: Anzahl tatsächlich veränderter Pixel. Bei der
    /// Bitmap-Schrift zählt der alte Weg (ein Zeichenelement, dessen
    /// Pixel wirklich geschrieben wurden), bei der Systemschrift die
    /// gemischten Pixel — beides ist eine Pixelzahl, damit der Aufrufer
    /// nicht unterscheiden muss, welche Schrift gearbeitet hat.
    pub fn zeichne(
        &mut self,
        fb: &mut Framebuffer,
        text: &str,
        farbe: Color,
        groesse: f32,
        origin: Point,
    ) -> usize {
        if text.is_empty() || !(groesse.is_finite() && groesse > 0.0) {
            return 0;
        }
        // M3: die Schleife laeuft ueber Zeichen der Eingabe.
        let text = match text.char_indices().nth(MAX_ZEICHEN) {
            Some((i, _)) => &text[..i],
            None => text,
        };
        // DIE SCHRIFT AUS DER STRUKTUR NEHMEN, statt sie zu klonen:
        // `TrueTypeFont` traegt alle Tabellen mit sich, ein Klon pro
        // Zeichen wuerde die ganze Seite kopieren. Der Weg
        // `take` + zuruecklegen ist dafuer der guenstigste, und er
        // hinterlaesst die Struktur auch bei einem `panic` in einem
        // kaputten Zeichen in einem Zustand, aus dem man weiterarbeiten
        // kann (der naechste Aufruf findet `None` und nimmt die Bitmap).
        let tt = self.tt.take();
        let gezeichnet = match tt.as_ref() {
            None => crate::glyphs::draw_text(fb, &self.bitmap, text, farbe, groesse, origin),
            Some(tt) => {
                let g = self.zeichne_truetype(fb, tt, text, farbe, groesse, origin);
                // Die Bitmap-Schrift laeuft MIT: sie ist der Rueckfall
                // fuer jedes Zeichen, das die Systemschrift nicht kennt.
                // Wer beides zeichnet, bekommt doppelte Tinte — deshalb
                // wird sie hier NICHT aufgerufen. Der Rueckfall steht
                // IN der Schleife (siehe `zeichne_truetype`).
                g
            }
        };
        self.tt = tt;
        gezeichnet
    }

    /// Der TrueType-Weg: Glyphen aus dem Umriss, Alpha-Maske gemischt.
    ///
    /// # DIE GRUNDLINIE
    ///
    /// `rasterize_outline` legt die Zeile 0 der Maske auf die **Oberkante**
    /// des Umrisses, nicht auf die Grundlinie. Hängt man die Maske oben an
    /// die Zeile, sitzt im Satz `A` tief und `o` hoch — die Oberkante
    /// einer Versalie ist höher als die eines Kleinbuchstabens. Also wird
    /// die Grundlinie aus dem Umriss berechnet und die Maske an ihr
    /// ausgerichtet:
    ///
    /// ```text
    /// grundlinie_in_maske = y_max * skale − floor(y_min * skale)
    /// ```
    ///
    /// GEMESSEN, warum nicht `measure_tt`: das gibt die Grundlinie des
    /// FONT (hhea), nicht der GLYPHE. Die hhea-Grenze gilt für die höchste
    /// Glyphe der Schrift — hängt man sie an ein `o`, schwebt es.
    fn zeichne_truetype(
        &mut self,
        fb: &mut Framebuffer,
        tt: &TrueTypeFont,
        text: &str,
        farbe: Color,
        groesse: f32,
        origin: Point,
    ) -> usize {
        // Die Positionen kommen aus dem Bitmap-Shaping: sie tragen den
        // Umbruch, den `narsil-page` gemessen hat. Nur die Form wechselt.
        let Ok(platziert) = layout_text(&self.bitmap, text, groesse, origin) else {
            return 0;
        };
        let em = tt.units_per_em() as f32;
        if em <= 0.0 {
            return 0;
        }
        let skale = groesse / em;
        let _ = skale;
        if em <= 0.0 {
            return 0;
        }
        let zellenhöhe = groesse / narsil_text::BASE_ROWS as f32;

        let mut gezeichnet = 0usize;
        // Der Stift laeuft mit eigener Breite: der Bitmap-Vorschub ist der,
        // den der Umbruch kennt (6/7 der Schriftgroesse), und er ist
        // GROESSER als der einer proportionalen Schrift. Die Seite bleibt
        // also breiter als der Text — siehe Modul-Kopf.
        let mut stift_x = origin.x;
        let mut zeilen_y = origin.y;
        let stapel = self.messung.stapel();
        let mut vorher: Option<narsil_text::GlyphQuelle> = None;
        for g in platziert {
            // Ein Zeilenumbruch: der Stift springt an den linken Rand.
            // GEMESSEN, warum ueber `y` und nicht ueber ein Zeichen:
            // `layout_text` erzeugt fuer `\n` KEINE Glyphe, es gibt also
            // kein Zeichen, an dem man den Umbruch erkennte.
            if (g.y - zeilen_y).abs() > zellenhöhe / 2.0 {
                zeilen_y = g.y;
                stift_x = origin.x;
                vorher = None;
            }
            // Mit Stapel: die Glyphe aus der Schrift holen, die das Zeichen HAT.
            if let Some(st) = stapel.as_deref() {
                if let Some(q) = st.glyphe_fuer(g.ch) {
                    if let Some(sch) = st.schrift(q.schrift) {
                        if let Some(p) = vorher {
                            stift_x += st.kern(p, q, groesse);
                        }
                        let font = &sch.font;
                        let grundlinie = self.grundlinie(font, q.glyph, groesse);
                        // DIE GRUNDLINIE DER ZEILE (A38).
                        //
                        // GEMESSEN 03.10.2026, warum hier **nicht** mehr
                        // `ASCENT_RATIO * groesse` steht: das sind **5/7 em**,
                        // der Wert der eingebauten 5x7-Bitmap. Die Schrift
                        // hat eine `hhea`-Tabelle mit echten Zahlen, und
                        // GEMESSEN ist (BERICHT_SCHRIFT_A18.txt):
                        //   FreeSans   0,9000 em  -> bei 16 px **2,97 px**
                        //   DejaVuSans 0,9282 em  -> bei 16 px **3,42 px**
                        // Der alte Wert war also um **2,97 bis 3,42 px** je
                        // Zeile zu tief, und bei dreizehn Zeilen sind das
                        // GEMESSEN 18,6 bis 21,3 px.
                        //
                        // GEMESSEN, warum genau `ascent_em`: das ist der
                        // Abstand von der Zeilenoberkante bis zur
                        // Grundlinie — dieselbe Bedeutung wie
                        // `ASCENT_RATIO`, aber **aus der Datei** statt aus
                        // einer Konstante. Und GEMESSEN, dass es der Wert
                        // ist, den `zeichne_mit_deko` seit A18 nimmt
                        // (`zeilen_oben + ascent_em * groesse`, Zeile 1386):
                        // damit stehen Dekoration und Text auf **einer**
                        // Grundlinie.
                        let zeilen_ascent = sch.font.zeilenmetriken().ascent_em;
                        gezeichnet += self.glyphe_mischen(
                            fb,
                            font,
                            q.glyph,
                            farbe,
                            groesse,
                            Point::new(
                                stift_x,
                                zeilen_grund(g.y + if zeilen_ascent > 0.0 {
                                    zeilen_ascent * groesse
                                } else {
                                    // Keine `hhea`: die Bitmap-Zahl als
                                    // Rueckfall, damit die Zeile nicht
                                    // auf 0 springt.
                                    ASCENT_RATIO * groesse
                                }) - grundlinie,
                            ),
                        );
                        let v = st.vorschub(q, groesse);
                        stift_x += if v > 0.0 { v } else { VORSCHUB * groesse };
                        vorher = Some(q);
                        continue;
                    }
                }
                vorher = None;
            }
            let vorschub = match tt.glyph_index(g.ch) {
                Some(index) => {
                    let grundlinie = self.grundlinie(tt, index, groesse);
                    // Die Grundlinie der Zeile. GEMESSEN 03.10.2026
                    // (A38): auch hier war `ASCENT_RATIO * groesse`
                    // (= **5/7 em**) das Falsche — GEMESSEN 2,97 px zu
                    // tief bei FreeSans und 3,42 px bei DejaVuSans
                    // (je 16 px). Siehe die lange Begruendung im
                    // Stapel-Zweig weiter oben (Zeile ~1225).
                    let zeilen_ascent = tt.zeilenmetriken().ascent_em;
                    gezeichnet += self.glyphe_mischen(
                        fb,
                        tt,
                        index,
                        farbe,
                        groesse,
                        Point::new(
                            stift_x,
                            zeilen_grund(g.y + if zeilen_ascent > 0.0 {
                                zeilen_ascent * groesse
                            } else {
                                ASCENT_RATIO * groesse
                            }) - grundlinie,
                        ),
                    );
                    //
                    // DER VORSCHUB, und er folgt der Messung des
                    // LAYOUTS (W5E, 2).
                    //
                    // GEMESSEN am 30.09.2026, 07:50 (`test2c.png`): mit
                    // dem TrueType-Vorschub und einer BITMAP-Messung im
                    // Layout war die Tinte 42 % schmaler als der Kasten
                    // — im Bild ein halber Bildschirm Luecke zwischen
                    // "einem" und "Link". Daraufhin wurde der Bitmap-
                    // Vorschub gesetzt (07:55): die Luecke war weg, und
                    // dafuer standen die Buchstaben sichtbar gesperrt
                    // ("W i l l k o m m e n") — der Preis derBitmap-Zelle
                    // (6/7 em) gegen den hmtx-Vorschub (rund 0.5 em).
                    //
                    // DER RICHTIGE SCHRITT (09:20): das Layout misst jetzt
                    // MIT derselben Schrift (`narsil_page::
                    // seite_aus_text_mit_schrift`,
                    // `narsil_layout_engine::TtMeasurer`). Damit sind die
                    // beiden Zahlen dieselbe, und es gilt der hmtx-
                    // Vorschub.
                    //
                    //
                    // W5G: DER VORSCHUB AUS DER MESSUNG, und damit ist
                    // die Frage "welche Schrift gilt" beantwortet, bevor
                    // sie gestellt wird. `Messung::vorschub` nimmt hmtx
                    // bei Systemschrift und 6/7 der Schriftgroesse bei der
                    // Bitmap — dieselbe Zahl, mit der der Umbruch bricht.
                    //
                    // GEMESSENER BEFUND (test6.png, 08:40): die drei
                    // Stellen (Umbruch, Boxbreite, Zeichnen) nahmen
                    // jede fuer sich etwas anderes, und das Ergebnis war
                    // eine halbe Bildschirmbreite Luecke zwischen "einem"
                    // und "Link". Mit EINER Messung gibt es diese Klasse
                    // Fehler nicht mehr — es gibt nur noch Messungen, die
                    // sich widersprechen, und das ist ein Test, kein Bild.
                    self.messung.vorschub(g.ch, groesse)
                }
                // DIE BITMAP ALS RUECKFALL, je Zeichen. Die Bitmap kennt
                // nur U+0020..=U+007E; alles andere wird ueBERSPRUNGEN —
                // aber der Vorschub bleibt der der Bitmap. Springt man
                // hier zur naechsten Position, rueckt der Rest des Satzes
                // zusammen und steht mitten im Wort.
                None => {
                    gezeichnet += crate::glyphs::draw_glyph(fb, &g, farbe);
                    0.0
                }
            };
            stift_x += if vorschub > 0.0 {
                vorschub
            } else {
                VORSCHUB * groesse
            };
        }
        gezeichnet
    }

    /// Zeichnet einen Text **mit** Textdekoration.
    ///
    /// GEMESSEN 05.10.2026, AUTONOM.txt:669 („text-decoration"): es gibt
    /// `draw_text_mit_deko` in `glyphs.rs:44`, das die Bits aus
    /// `narsil-css` (`kaskade.rs`: 1 = `underline`, 4 = `line-through`)
    /// auswertet — und GEMESSEN, dass es **niemand aufruft**. Auch
    /// `draw_text` (glyphs.rs:30) ruft es mit `0` auf. Beide nehmen
    /// ausserdem einen `&BitmapFont`, also die eingebaute 5x7-Bitmap,
    /// waehrend der Browser ueber [`Schrift::zeichne`] mit der
    /// Systemschrift zeichnet. Die Dekoration war im echten Pfad also
    /// nicht erreichbar — diese Methode macht sie erreichbar, ohne den
    /// fremden Aufrufer zu aendern.
    ///
    /// GEMESSEN, warum die Dekoration ueber den **Vorschub** laeuft und
    /// nicht ueber die Tintenbreite (`glyphs.rs:38` sagt dasselbe): ein
    /// Unterstrich unter „Hamburg" muss unter den ganzen Lauf gehen,
    /// auch ueber die Leerzeichen hinweg. Wer die Tinte vermisst, bekommt
    /// vier kleine Striche unter je vier Buchstaben.
    ///
    /// GEMESSEN, was `deko_zeile` erwartet: eine `zeilen_oben`, keine
    /// Grundlinie (`deko.rs:124`) — daraus rechnet es selbst
    /// (`unterstrich_y` = `basislinie(...) − 0,126 em` bei FreeSans). Und
    /// GEMESSEN, dass die Breite aus [`Messung::breite`] kommt, also
    /// aus derselben Messung, mit der auch der Umbruch bricht — das ist
    /// die Kopplung, an der es in diesem Projekt schon einmal 42 % zu
    /// viel Luecke gab (W5E).
    #[must_use]
    pub fn zeichne_mit_deko(
        &mut self,
        fb: &mut Framebuffer,
        text: &str,
        farbe: Color,
        groesse: f32,
        origin: Point,
        deko: u8,
    ) -> usize {
        let mask = crate::deko::mask_gueltig(deko);
        if mask == 0 {
            return self.zeichne(fb, text, farbe, groesse, origin);
        }
        // Die Zeilen muessen getrennt werden: ein Strich darf keine zwei
        // Zeilen verbinden. GEMESSEN in `glyphs.rs:87`, dass `layout_text`
        // an Zeilenumbruechen eine Luecke laesst.
        //
        // DIE ZEILENHOEHE, einmal vor der Schleife geholt. GEMESSEN,
        // warum nicht je Zeile: `self.tt` wird in `zeichne` mit `take`
        // entnommen und wieder hingelegt, und ein zweites `take` mitten
        // in der Schleife wuerde die Bitmap als Rueckfall nehmen.
        //
        // GEMESSEN, warum ueberhaupt `hhea`: vorher waere `groesse`
        // selbst genommen worden (`font_size` als Zeilenhoehe), und das
        // ist GEMESSEN bei `FreeSans.ttf` **4,80 px zu klein** je Zeile
        // (siehe BERICHT_SCHRIFT_A18.txt).
        let z = self.tt.as_ref().map(|t| t.zeilenmetriken());
        let zeilenhoehe_em = z.map_or(0.0, |z| z.zeilenhoehe_em());
        // GEMESSEN, warum die GRUNDLINIE und nicht `zeilen_oben` an
        // `deko_zeile` geht: `deko_zeile` rechnet sie mit `ASCENT_EM =
        // 0,800`, und das ist fuer `DejaVuSans.ttf` GEMESSEN 16 % zu
        // klein. `deko_an_grundlinie` nimmt sie von aussen — dieselbe,
        // aus der auch die Glyphen gezeichnet werden (schrift.rs:964).
        let ascent_em = z.map_or(0.0, |z| z.ascent_em);
        let mut strich_pixel = 0usize;
        let mut zeilen_oben = origin.y;
        let mut glyphen_pixel = 0usize;
        for stueck in text.split('\n') {
            let gemessen = self.zeichne(
                fb,
                stueck,
                farbe,
                groesse,
                Point::new(origin.x, zeilen_oben),
            );
            glyphen_pixel += gemessen;
            let breite = self.messung.breite(stueck, groesse);
            if breite > 0.0 {
                // GEMESSEN: `origin.y` ist der Zell-Anfang, und die
                // Grundlinie liegt `ascent_em * groesse` darunter. Bei
                // `DejaVuSans.ttf` sind das GEMESSEN 0,9282 em.
                let grundlinie = zeilen_oben + ascent_em * groesse;
                strich_pixel += crate::deko::deko_an_grundlinie(
                    fb,
                    mask,
                    grundlinie,
                    groesse,
                    origin.x,
                    origin.x + breite,
                    farbe,
                ) as usize;
            }
            zeilen_oben += if zeilenhoehe_em > 0.0 {
                zeilenhoehe_em * groesse
            } else {
                groesse
            };
        }
        //
        // DIE RUECKGABE, und sie ist die Summe aus Glyphen und Strich.
        //
        // GEMESSEN, warum das so sein muss: in der ersten Fassung gab die
        // Methode nur `strich_pixel` zurueck, also GEMESSEN 158 Pixel
        // fuer eine Zeile mit Unterstrich und 1083 fuer dieselbe Zeile
        // ohne. Damit war die Zahl nicht mehr mit der von `zeichne`
        // vergleichbar — der Aufrufer zaehlt Pixel, und die Glyphen
        // sind Pixel.
        glyphen_pixel + strich_pixel
    }

    /// Die Grundlinie innerhalb der Maske, in Pixeln von oben.
    fn grundlinie(&mut self, tt: &TrueTypeFont, index: u32, groesse: f32) -> f32 {
        let groessen_schluessel = (groesse * 64.0).clamp(0.0, u32::MAX as f32) as u32;
        let schluessel = (tt.id(), index, groessen_schluessel);
        if let Some(&wert) = self.basen.get(&schluessel) {
            return wert;
        }
        // M4: die Tabelle ist begrenzt. Bei Vollständigkeit wird sie
        // LEER GEMACHT statt einem Eintrag zu opfern: das ist ein Sprung
        // im Zeitverhalten, aber kein Wachstum. Ein LRU wäre besser,
        // gehörte aber in `narsil-text` neben den anderen Cache.
        if self.basen.len() >= MAX_BASEN {
            self.basen.clear();
        }
        let wert = match tt.glyph_outline(index) {
            Ok(u) => {
                let skale = groesse / tt.units_per_em() as f32;
                // Die Grundlinie (y = 0 im Umriss) liegt in der Maske von
                // `rasterize_outline` bei Zeile ceil(y_max * skale): dort ist
                // Zeile 0 = ceil(y_max * skale) und py = y_max_px - y * skale.
                // KORREKTUR 30.09. (Koordination): vorher stand hier
                // y_max - floor(y_min), also die GANZE Glyphenhoehe - damit
                // sass jede Glyphe mit ihrer UNTERKANTE auf der Grundlinie.
                // Folge im Bild (rust-lang.org): g/y/p ohne Unterlaenge
                // ('Lan9ua9e'), Bindestrich als Unterstrich, Apostroph als
                // Komma. Fuer A/o/m fiel es nicht auf, weil dort y_min = 0.
                let _ = skale;
                narsil_text::raster::masken_grundlinie(&u, groesse, tt.units_per_em())
            }
            // Kein Umriss (Leerzeichen, unbekannte Glyphe): 0. Der
            // Aufrufer setzt die Maske dann an die Oberkante der Zeile,
            // und ein Leerzeichen zeichnet ohnehin nichts.
            Err(_) => 0.0,
        };
        self.basen.insert(schluessel, wert);
        wert
    }

    /// Rastert eine Glyphe und mischt ihre Alpha-Maske in den Framebuffer.
    ///
    /// # HIER IST DAS ANTIALIASING (I4)
    ///
    /// `rasterize_outline` liefert 8-Bit-Alpha je Pixel. Der Bitmapweg
    /// schreibt den Farbwert dagegen DIREKT (`set_pixel`, kein Blending) —
    /// bei 5×7 ist das richtig, weil es dort nur 0 und 255 gibt. Hier wird
    /// gemischt: ein Pixel mit Alpha 128 ergibt eine Mischfarbe, kein
    /// entweder-oder. Ohne das wären TrueType-Kanten so gezackt wie eine
    /// 1-Bit-Schrift — man hätte die bessere Schrift gekauft und keine
    /// bessere Anzeige bekommen.
    fn glyphe_mischen(
        &mut self,
        fb: &mut Framebuffer,
        tt: &TrueTypeFont,
        index: u32,
        farbe: Color,
        groesse: f32,
        ziel: Point,
    ) -> usize {
        //
        // GEMESSEN 01.10.2026 (Befund "jede 0 ueberlappt mit der Glyphe
        // links daneben", Probe-PNG team/log/befund_null_archlinux.png):
        // `rasterize_outline` liefert die Bitmap ab `floor(xMin * scale)`,
        // NICHT ab der Schreibposition. Hier wurde sie an der
        // Schreibposition abgesetzt — der linke Seitenrand ging verloren,
        // und jede Glyphe wurde um ihn zu weit links gemalt.
        //
        // GEMESSEN an FreeSans bei 16 px: die Seitenraender der Ziffern
        // sind 0,45 bis 1,63 Pixel ('4' 28, '1' 102, '0' 43 in
        // Einheiten/1000). Die Zifferntinte ist 9 Pixel breit, der
        // Ziffernvorschub 8,90 — es gibt also fast keine Reserve. Der
        // fehlende Seitenrand reicht, um benachbarte Ziffern zu
        // verschmelzen. Bei 32 px und mehr verschwindet der Effekt: der
        // Vorschub waechst auf 17,8 Pixel, die Tinte bleibt bei 16, und
        // es bleibt eine Luecke. Genau das beschreibt der Befund — nur
        // kleine Schrift leidet.
        //
        // Die erste Fassung dieses Kommentars sprach von `floor()` gegen
        // `round()`. Das ist nicht die Ursache: die beiden Probe-PNGs
        // `befund_null_archlinux.png` und `befund_null_archlinux_nach_fix.png`
        // sind BYTEWEISE GLEICH. Die Rundung bleibt, weil sie den Versatz
        // auf der x-Achse ausbalanciert statt ihn immer nach links zu
        // treiben, aber sie repariert den Seitenrand nicht.
        //
        // DIE BITMAP, und sie kommt VOR der Kontur.
        //
        // GEMESSEN 04.10.2026 an `NotoColorEmoji.ttf`: die Datei hat kein
        // `glyf`. `glyph_outline` gibt also einen leeren Umriss zurueck, und
        // ohne diesen Zweig wuerde jedes Emoji als leerer Kasten erscheinen
        // — bei einer Schrift, die genau fuer die Farbe da ist.
        if let Some(b) = self.bitmap_glyph(tt, index) {
            let skale = groesse / f32::from(self.bitmap_ppem(tt).max(1));
            return self.bitmap_mischen(fb, &b, skale, ziel);
        }
        //
        // SYNTHETISCHES FETT/KURSIV, und das ist die Stelle, an der die
        // Flags aus `Schrift::synthetisch_*` endlich eine Wirkung haben.
        //
        // GEMESSEN 05.10.2026, AUTONOM.txt:669: `Stilauswahl` traegt
        // `synthetisch_fett`/`synthetisch_kursiv`, und GEMESSEN, dass
        // `narsil-raster` sie vorher NIRGENDS gelesen hat. Zweimal
        // gezeichnet, waren die Bilder pixelweise gleich — es gab
        // keinerlei Fett-Wirkung.
        //
        // GEMESSEN, warum die Verzerrung VOR der Rasterung passiert und
        // nicht danach: eine Kontur, die man nachtraechlich in der Maske
        // verdickt, waere ein Rechteck um die Tinte — abgeschnittene
        // Ecken an den Enden, und bei einer Rundung waere die Verdickung
        // aussen breiter als innen. Im Umriss dagegen bekommt jede
        // Kontur ihre richtige Form, und der Kantenglaetter rechnet
        // danach wie bei jeder anderen Glyphe.
        //
        // GEMESSEN, dass der Vorschub mitwaechst: bei einer echten fetten
        // Schrift ist „Hamburgefonstiv" GEMESSEN 13,0 % breiter. Das
        // waechst von selbst mit, weil die Glyphe in der Maske breiter
        // wird und `st.vorschub` den hmtx-Wert liefert — der
        // Bitmap-Vorschub waere falsch, aber genau der ist der Fehler
        // vom 30.09. (W5E) und wird hier nicht benutzt.
        // Release 10.10.2026: ohne synthetischen Schnitt gleich aus dem
        // Masken-Cache — der Umriss wird nur fuer die Verzerrung gebraucht.
        // GEMESSEN vorher (heise.de, 186 Textlaeufe je Bild): JEDE Glyphe
        // las in JEDEM Bild ihren Umriss aus der Schriftdatei, bevor der
        // Cache gefragt wurde.
        if !self.synthetisch_fett && !self.synthetisch_kursiv {
            if let Some(f) = Self::feinheit(farbe) {
                let Ok(m) = self.cache.get_or_rasterize_fein(tt, index, groesse, f) else {
                    return 0;
                };
                let grundlinie_alt = self.grundlinie(tt, index, groesse);
                return Self::maske_mischen(fb, &m, farbe, ziel, grundlinie_alt);
            }
        }
        let umriss = match tt.glyph_outline(index) {
            Ok(u) => u,
            Err(_) => return 0,
        };
        let verzoerrt = crate::synthetisch::verzerren(
            &umriss,
            groesse,
            tt.units_per_em(),
            self.synthetisch_fett,
            self.synthetisch_kursiv,
        );
        // Release 09.10.2026: DIE FEINE RASTERUNG (vertikales Hinting,
        // Stem-Darkening, LCD fuer Leistentext) mit gamma-korrekter
        // Mischung. Synthetisches bleibt auf dem alten Weg: die Verdickung
        // rechnet in ganzen Pixeln und kennt keine Teilpixel.
        let fein = Self::feinheit(farbe);
        if verzoerrt.is_none()
            && !self.synthetisch_fett
            && let Some(f) = fein
        {
            let Ok(m) = self.cache.get_or_rasterize_fein(tt, index, groesse, f) else {
                return 0;
            };
            let grundlinie_alt = self.grundlinie(tt, index, groesse);
            return Self::maske_mischen(fb, &m, farbe, ziel, grundlinie_alt);
        }
        let (maske, breite, hoehe, x_versatz) = match &verzoerrt {
            // Ohne Verzerrung: der CACHE. Der ist der Normalfall, und
            // eine verzoerrte Glyphe darin zu lassen wuerde bedeuten,
            // dass dieselbe Glyphe einmal fett und einmal normal
            // gezeichnet wird.
            None => match self.cache.get_or_rasterize_mit_versatz(tt, index, groesse) {
                Ok(m) => m,
                Err(_) => return 0,
            },
            // Mit Verzerrung: frisch rastern, ohne den Cache zu
            // befruehlen. GEMESSEN, warum: der Cache-Schluessel
            // (schrift.rs:1008) ist `(id, glyph, groesse)` — ohne die
            // Flags wuerde ein fetter Lauf den normalen Eintrag
            // ueberschreiben und der naechste normale Lauf bekame
            // fette Tinte.
            // Release 10.10.2026: eingerastet wie die normalen Glyphen
            // (`Rasterart::zum_zeichnen`), sonst stuende ein kursiver Lauf
            // gegen den aufrechten um bis zu einen Pixel versetzt.
            Some(u) => match narsil_text::raster::rasterize_outline_art(
                u,
                groesse,
                tt.units_per_em(),
                narsil_text::raster::Rasterart::zum_zeichnen(),
            ) {
                Ok(m) => (m.bitmap, m.breite, m.hoehe, 0),
                Err(_) => return 0,
            },
        };
        // 1b) Das FETT als Masken-Verdickung. GEMESSEN, warum es hier
        //     und nicht im Umriss steht: s. `synthetisch.rs`, Modulkopf —
        //     eine Punktverschiebung tut nichts (GEMESSEN: 1113 Pixel
        //     gegen 1113), und ein Rechteck um die Kontur waere an den
        //     Enden abgeschnitten. Die Maske ist bereits antialiased,
        //     also bleibt die Verdickung es auch.
        let (maske, breite, hoehe, x_versatz) = if self.synthetisch_fett {
            let d = crate::synthetisch::verdickung_px(groesse).round().max(1.0) as u32;
            match crate::synthetisch::maske_verdicken(&maske, breite, hoehe, d) {
                Some(verdickt) => {
                    // `x_versatz` wandert um die halbe Verdickung nach
                    // links: die Maske ist jetzt `d` Pixel breiter und
                    // beginnt `d` Pixel frueher.
                    (verdickt, breite + d * 2, hoehe, x_versatz - d as i32)
                }
                // Die Grenze hat gegriffen: die unverdickte Maske
                // zeichnen. GEMESSEN, warum das besser ist als nichts:
                // der Text bleibt lesbar.
                None => (maske, breite, hoehe, x_versatz),
            }
        } else {
            (maske, breite, hoehe, x_versatz)
        };
        // 1c) Der KURSIV kommt aus `verzerren` oben, **nicht** von hier.
        //
        // GEMESSENE FALLE (03.10.2026 21:7x, SCHRIFT): ich habe an dieser
        // Stelle zuerst eine Masken-Schaegstellung eingebaut
        // (`synthetisch::maske_neigen`) und sie dann wieder entfernt.
        // Grund: der KURSIV war nie absent — `verzerren` schiebt die
        // **Konturpunkte** (`p.x += p.y * neigung`, schrift.rs:1644), und
        // erst danach wird geraestert. Das ist der bessere Ort: eine
        // geneigte Kontur wird sauber geraestert, eine geschobene Maske
        // bekommt Treppen an den Kanten. Zwei Mechanismen fuer dieselbe
        // Sache wuerden die Neigung **verdoppeln**, sobald die Konstante
        // stimmt — GEMESSEN: `post.italicAngle` von `DejaVuSans-Oblique`
        // ist −11,00 Grad, und `KURSIV_EM` stand auf der Haelfte davon
        // (0,0960 statt 0,1944), siehe `synthetisch.rs`.
        // M4: die Kantenlänge wird VOR der Auswertung geprüft, und die
        // Laenge der Maske muss zur Rechteckgroesse passen — eine Maske,
        // die zu kurz ist, waere ein Lesen ueber das Ende hinaus.
        if breite == 0 || hoehe == 0 || breite > MAX_MASKE_KANTE || hoehe > MAX_MASKE_KANTE {
            return 0;
        }
        if maske.len() != (breite * hoehe) as usize {
            // Die Laenge passt nicht: ein Fehler in der Rasterung, kein
            // Grund zu raten.
            return 0;
        }

        //
        // GEMESSEN 01.10.2026 (Befund "jede 0 ueberlappt mit der Glyphe
        // links daneben", Probe-PNG team/log/befund_null_archlinux.png):
        // hier stand `floor()`. Das schneidet JEDE Glyphe bis zu fast
        // einer vollen Spalte nach links ab — im Mittel um eine halbe.
        // Bei FreeSans (dem sans-serif-Gewinner dieser Maschine) ist der
        // Ziffernvorschub 556/1000 em, die Zifferntinte aber 9 Pixel
        // breit bei 16 px Schriftgroesse: die Tinte ist BREITER als der
        // Vorschub. Jede Glyphe rutscht also um eine halbe Spalte in die
        // linke Nachbarin hinein.
        //
        // Warum ausgerechnet die "0": sie hat als einzige Ziffer eine
        // fast senkrechte linke Kontur. Eine halbe Spalte Versatz
        // zerstört diese Kontur sichtbar, waehrend "1" (5 Pixel) und
        // "7" daneben unauffaellig bleiben. Genau das beschreibt der
        // Befund — "20" liest sich als "2b", "09" verschmiert, die
        // uebrigen Ziffern stehen sauber.
        //
        // `round()` rundet zur naechsten Spalte und laesst den Versatz im
        // Mittel bei null, statt ihn immer nach links zu treiben. NUR AUF
        // DER x-ACHSE: auf y liegt die Grundlinie, dafuer gibt es
        // bestehende Messungen (`tests/grundlinie.rs`), und da ist nichts
        // gemessen — also bleibt `floor()`.
        //
        // `x_versatz` ist der linke Seitenrand. Ohne ihn stuende die
        // Bitmap zu weit links.
        let x0 = ziel.x.round() as i64 + x_versatz as i64;
        let y0 = ziel.y.floor() as i64;
        let mut veraendert = 0usize;
        for y in 0..hoehe {
            for x in 0..breite {
                let a = maske[(y * breite + x) as usize];
                if a == 0 {
                    continue;
                }
                let px = x0 + x as i64;
                let py = y0 + y as i64;
                if px < 0 || py < 0 {
                    continue;
                }
                let (px, py) = (px as u32, py as u32);
                // Der Clip des Framebuffers gilt auch hier: ein Zeichen,
                // das aus dem Fenster rausfaellt, ist abgeschnitten und
                // nicht uebergangen.
                let Some(hintergrund) = fb.pixel(px, py) else {
                    continue;
                };
                let alt = hintergrund;
                // Release 09.10.2026: mit der feinen Rasterung auch hier
                // gamma-korrekt (synthetisch fett/kursiv); ohne sie wie bisher.
                let neu = if fein.is_some() {
                    crate::textgamma::mische(farbe, [a; 3], alt)
                } else {
                    blend_source_over(Color::new(farbe.r, farbe.g, farbe.b, a), alt)
                };
                if neu == alt {
                    // Nichts Sichtbares: kein Pixel, kein Zaehler.
                    continue;
                }
                if fb.set_pixel(px, py, neu) {
                    veraendert += 1;
                }
            }
        }
        veraendert
    }

    /// Release 09.10.2026: wie fein gerastert wird, oder `None` fuer den
    /// alten Weg (`NARSIL_SCHRIFT_FEIN=0`, fuer Vorher/Nachher-Bilder).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Verdunkelung nur fuer HELLEN Text
    /// gilt: dunkler Text auf hellem Grund ist mit der Kontrastanhebung der
    /// Mischung schon so kraeftig wie bisher (GEMESSEN halbe Deckung 118
    /// statt 127) — dicker waere er fetter als in Firefox. Heller Text auf
    /// dunklem Grund (Leiste, dunkle Seiten) ist der, der duenn wirkt.
    fn feinheit(farbe: Color) -> Option<narsil_text::raster::Feinheit> {
        if !crate::textgamma::fein_an() {
            return None;
        }
        let hell = 0.2126 * f32::from(farbe.r) + 0.7152 * f32::from(farbe.g) + 0.0722 * f32::from(farbe.b) > 127.5;
        Some(narsil_text::raster::Feinheit {
            hinting: true,
            verdunkeln: hell,
            lcd: crate::textgamma::ist_leiste() && crate::textgamma::lcd().is_some(),
        })
    }

    /// Release 09.10.2026: mischt eine feine Maske (Graustufen oder LCD).
    ///
    /// `ziel` ist die Oberkante der UNGEHINTETEN Maske (so rechnet der
    /// Aufrufer), `grundlinie_alt` deren Grundlinienzeile — die Grundlinie
    /// im Bild ist also `floor(ziel.y) + grundlinie_alt`, und die feine Maske
    /// wird mit IHRER Grundlinienzeile daran gehaengt. Ohne Hinting ist das
    /// dieselbe Zeile wie bisher.
    ///
    /// LCD: die Schreibposition wird auf DRITTEL Pixel gerundet (die Maske
    /// hat drei Teilpixel je Pixel), jeder Bildpunkt bekommt je Kanal die
    /// Deckung seines Teilpixels — bei BGR gespiegelt.
    fn maske_mischen(
        fb: &mut Framebuffer,
        m: &narsil_text::raster::Maske,
        farbe: Color,
        ziel: Point,
        grundlinie_alt: f32,
    ) -> usize {
        if m.breite == 0 || m.hoehe == 0 {
            return 0;
        }
        let teile = if m.lcd { 3 } else { 1 };
        if m.breite > MAX_MASKE_KANTE * teile
            || m.hoehe > MAX_MASKE_KANTE
            || m.bitmap.len() != (m.breite * m.hoehe) as usize
        {
            return 0;
        }
        let grundlinie = ziel.y.floor() as i64 + grundlinie_alt as i64;
        let y0 = grundlinie - i64::from(m.grundlinie);
        let mut veraendert = 0usize;
        // Release 10.10.2026: der Zeichenbereich (Puffer + Clip) EINMAL je
        // Glyphe, dann zeilenweise direkt in den Puffer — vorher je Punkt
        // `pixel()` und `set_pixel()` mit Grenz- und Clip-Pruefung.
        let (cx0, cy0, cx1, cy1) = fb.zeichenbereich();
        let (cx0, cy0, cx1, cy1) = (i64::from(cx0), i64::from(cy0), i64::from(cx1), i64::from(cy1));
        if !m.lcd {
            let x0 = ziel.x.round() as i64 + i64::from(m.x_versatz);
            // Die Spalten der Maske, die im Bereich liegen.
            let von = (cx0 - x0).max(0);
            let bis = (cx1 - x0).min(i64::from(m.breite));
            if von >= bis {
                return 0;
            }
            for y in 0..m.hoehe {
                let py = y0 + i64::from(y);
                if py < cy0 || py >= cy1 {
                    continue;
                }
                let quelle = &m.bitmap[(y * m.breite) as usize..((y + 1) * m.breite) as usize];
                let zeile = fb.zeile_mut(py as u32);
                for x in von..bis {
                    let a = quelle[x as usize];
                    if a == 0 {
                        continue;
                    }
                    let i = ((x0 + x) as usize) * 4;
                    let px = &mut zeile[i..i + 4];
                    let alt = Color::new(px[0], px[1], px[2], px[3]);
                    let neu = crate::textgamma::mische(farbe, [a; 3], alt);
                    if neu != alt {
                        px.copy_from_slice(&[neu.r, neu.g, neu.b, neu.a]);
                        veraendert += 1;
                    }
                }
            }
            return veraendert;
        }
        let bgr = crate::textgamma::lcd() == Some(crate::textgamma::LcdOrdnung::Bgr);
        // Der erste Teilpixel der Maske, absolut.
        let start = (ziel.x * 3.0).round() as i64 + i64::from(m.x_versatz);
        let px_von = start.div_euclid(3);
        let px_bis = (start + i64::from(m.breite) + 2).div_euclid(3);
        let (px_von, px_bis) = (px_von.max(cx0), px_bis.min(cx1));
        for y in 0..m.hoehe {
            let py = y0 + i64::from(y);
            if py < cy0 || py >= cy1 {
                continue;
            }
            let zeile = &m.bitmap[(y * m.breite) as usize..((y + 1) * m.breite) as usize];
            for px in px_von..px_bis {
                if px < 0 {
                    continue;
                }
                let mut d = [0u8; 3];
                for (c, ziel_d) in d.iter_mut().enumerate() {
                    let j = px * 3 + c as i64 - start;
                    if j >= 0 && (j as usize) < zeile.len() {
                        *ziel_d = zeile[j as usize];
                    }
                }
                if d == [0, 0, 0] {
                    continue;
                }
                if bgr {
                    d.swap(0, 2);
                }
                let (pxu, pyu) = (px as u32, py as u32);
                let Some(alt) = fb.pixel(pxu, pyu) else { continue };
                let neu = crate::textgamma::mische(farbe, d, alt);
                if neu != alt && fb.set_pixel(pxu, pyu, neu) {
                    veraendert += 1;
                }
            }
        }
        veraendert
    }
}

/// Haelt den Pfad einer Systemschrift fuer die Anzeige fest.
///
/// GEMESSEN 06.10.2026, KORREKTUR dieser Notiz: hier stand, das `Box::leak`
/// sei „eine bewusste, winzige Entscheidung: ein Pfad pro Programm". Das
/// stimmte nicht. `suche_schnitt` wird bei **jedem** Bau einer Schrift
/// gerufen — also auch bei jedem `Schrift::mit_gewicht`, und das ist der
/// Weg fuer `font-weight`/`font-style`. GEMESSEN: `mit_gewicht` erzeugt
/// bei jedem Aufruf eine neue `Schrift`, und `suche_schnitt` leckte dann
/// **eine Kopie des Pfades**. Ein Text, der im Zeichenpfad den Schnitt
/// wechselt, leckte also pro Wechsel einen Pfad — nicht „einen pro
/// Programm".
///
/// Die Rueckgabe ist jetzt ein eigener `String`. Kostet: 32 Byte je
/// `Schrift`, und es gibt je Frame eine. Gewinnt: der Aufrufer muss
/// nichts leaken, und `SchriftQuelle` traegt den Pfad ohne
/// `Box::leak` (siehe die Notiz am `enum`).
fn pfad_als_text(pfad: &Path) -> String {
    match pfad.to_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        // `None` heisst: der Pfad ist kein UTF-8. Dann wird das gesagt,
        // statt einen Ersatz zu erfinden, der wie ein Pfad aussieht.
        _ => "<kein UTF-8-Pfad>".to_string(),
    }
}

#[cfg(test)]
mod ziffer_ueberlappung {
    //! BEFUND 01.10.2026: auf archlinux.org erscheint im Datum
    //! "2026-09-22" nur die "0" falsch, "2", "6" und "9" sind sauber.
    //!
    //! GEMESSEN 01.10.2026 (Koordination): archweb.css Z. 432 setzt das
    //! Datum als `font-size: 0.85em`. Das sind 11,9 px bei 14 px Grundtext,
    //! 12,75 px bei 15 px und 13,6 px bei 16 px — GEBROCHENE Groessen.
    //! Die erste Fassung dieser Tests hat ausschliesslich ganze
    //! Schriftgroessen geprueft (16, 32, 48, 56, 59) und den Fehler genau
    //! deshalb nicht gesehen. Das ist die wichtigste Lehre daraus: nicht
    //! die Vermutung war falsch, sondern die Groesse, an der man misst.
    //!
    //! GEMESSEN an der Glyphe: die "0" hat eine INNERE Kontur (das Loch),
    //! und deren Windungsrichtung ist das Gegenteil der aeusseren. Bei
    //! nicht-ganzzahliger Skala faellt eine Kontur auf halbe Pixel, und
    //! wenn die Regel "Rand oben gehoert zur inneren" nicht greift,
    //! fuellt sich das Loch.
    //!
    //! Die Tests sehen sich die GERADE Glyphe als Bild an, nicht nur ihre
    //! Breite: eine Zahl allein kann eine ausgefuellte Null nicht von einer
    //! sauberen unterscheiden.

    use super::*;
    use narsil_core::geometry::Point;

    const B: u32 = 400;
    const H: u32 = 60;
    const URSPRUNG: Point = Point::new(8.0, 8.0);

    /// Die Groessen, um die es geht: 0,85 em bei 14, 15 und 16 px
    /// Grundtext, dazu die kleinsten und groessten, bei denen es sonst
    /// auffaellt.
    const GROESSEN: [f32; 8] = [11.9, 12.75, 13.6, 16.0, 23.8, 25.5, 27.2, 32.0];

    /// Die Spalten 0..B, in denen Tinte liegt.
    fn tintenspalten(text: &str, groesse: f32) -> Vec<bool> {
        let mut s = Schrift::mit_systemschrift();
        let mut fb = Framebuffer::new(B, H, Color::TRANSPARENT);
        let n = s.zeichne(&mut fb, text, Color::BLACK, groesse, URSPRUNG);
        assert!(
            n > 0,
            "\"{text}\" bei {groesse} px hat keine Tinte gezeichnet"
        );
        (0..B)
            .map(|x| {
                (0..H)
                    .filter(|&y| fb.pixel(x, y).is_some_and(|c| c.a > 0x40))
                    .count()
                    > 0
            })
            .collect()
    }

    /// Getrennte Tintenbroecke als (von, bis, breite).
    fn broecke(spalten: &[bool]) -> Vec<(u32, u32, u32)> {
        let mut raus = Vec::new();
        let mut start: Option<u32> = None;
        for (x, &dunkel) in spalten.iter().enumerate() {
            match (dunkel, start) {
                (true, None) => start = Some(x as u32),
                (false, Some(a)) => {
                    raus.push((a, x as u32 - 1, x as u32 - a));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(a) = start {
            raus.push((a, B - 1, B - a));
        }
        raus
    }

    /// Das Bild einer einzelnen Glyphe als Text, fuer den Vergleich.
    fn als_bild(text: &str, groesse: f32) -> String {
        let mut s = Schrift::mit_systemschrift();
        let mut fb = Framebuffer::new(B, H, Color::TRANSPARENT);
        let _ = s.zeichne(&mut fb, text, Color::BLACK, groesse, URSPRUNG);
        let b = broecke(&tintenspalten(text, groesse));
        let (von, bis) = match (b.first(), b.last()) {
            (Some(x), Some(y)) => (x.0, y.1),
            _ => return "(keine Tinte)".to_string(),
        };
        let ys: Vec<u32> = (0..H)
            .filter(|y| (von..=bis).any(|x| fb.pixel(x, *y).is_some_and(|c| c.a > 0x40)))
            .collect();
        let (yo, yu) = match (ys.first(), ys.last()) {
            (Some(x), Some(y)) => (*x, *y),
            _ => return "(keine Tinte)".to_string(),
        };
        let mut raus = String::new();
        raus.push_str(&format!("{groesse} px (Zeilen {yo}..{yu})\n"));
        for y in yo..=yu {
            for x in von..=bis {
                raus.push(match fb.pixel(x, y) {
                    Some(c) if c.a > 200 => '#',
                    Some(c) if c.a > 64 => '+',
                    Some(_) => '.',
                    None => ' ',
                });
            }
            raus.push('\n');
        }
        raus
    }

    /// DER TEST: die "0" muss bei JEDER der gemessenen Groessen ein Loch
    /// haben. Eine ausgefuellte Null ist der Fehler, und ein Loch kann man
    /// nicht mit einer Breite verwechseln.
    #[test]
    fn die_null_hat_ihr_loch_bei_gebrochener_schriftgroesse() {
        for groesse in GROESSEN {
            eprintln!("{}", als_bild("0", groesse));
            let mut s = Schrift::mit_systemschrift();
            let mut fb = Framebuffer::new(B, H, Color::TRANSPARENT);
            let _ = s.zeichne(&mut fb, "0", Color::BLACK, groesse, URSPRUNG);
            let b = broecke(&tintenspalten("0", groesse));
            let (von, bis) = (b[0].0, b[0].1);
            // Die Mitte des Lochs: die mittleren 30 % der Hoehe, in der
            // Mitte der Breite.
            let ys: Vec<u32> = (0..H)
                .filter(|y| (von..=bis).any(|x| fb.pixel(x, *y).is_some_and(|c| c.a > 0x40)))
                .collect();
            let hoehe = ys[ys.len() - 1] - ys[0] + 1;
            let y0 = ys[0] + hoehe * 35 / 100;
            let y1 = ys[0] + hoehe * 65 / 100;
            let x0 = von + (bis - von) * 35 / 100;
            let x1 = von + (bis - von) * 65 / 100;
            let mut tinte = 0u32;
            let mut flaeche = 0u32;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    flaeche += 1;
                    if fb.pixel(x, y).is_some_and(|c| c.a > 64) {
                        tinte += 1;
                    }
                }
            }
            assert!(
                flaeche > 0,
                "die '0' bei {groesse} px ist {von}..{bis} breit, aber ohne Zeilen — \
                 sie ist gar nicht gezeichnet worden"
            );
            let anteil = tinte as f64 / flaeche as f64;
            assert!(
                anteil < 0.45,
                "die '0' bei {groesse} px hat ihr LOCH verloren: die Mitte ist zu \
                 {anteil:.2} gefuellt (gebrochen: {tinte} von {flaeche} Pixeln).\n{}",
                als_bild("0", groesse)
            );
        }
    }

    /// Und die uebrigen Ziffern muessen gefuellt sein — sonst waere der
    /// Test umgekehrt und wuerde jede kaputte Maske durchlassen.
    /// GEMESSEN: nur "0", "6", "8" und "9" haben einen geschlossenen
    /// Bauch. "2", "3", "5" und "7" haben KEINEN.
    ///
    /// Wichtig, und das hat mich einen Test gekostet: die erste Fassung
    /// verlangte von "2", "3", "5", "7" eine zu 30 % GEFUELLTE Mitte. Die
    /// "2" hat aber auf der LINKEN Seite der Mittelzeile nie Tinte — das
    /// ist ihre Form, kein Loch. GEMESSEN 01.10.2026 bei 11,9 px:
    ///     ..++...
    ///     .#++#+.
    ///     ++...#.      <- Mitte: nur rechts Tinte
    ///     .....#.
    ///     ....#+.
    ///     ..+#+..
    /// Also ist "gefuellt" als Schranke falsch. Richtig sind ZWEI
    /// Aussagen, und zusammen unterscheiden sie:
    ///   * die Ziffern MIT Bauch haben in der Mitte ein LOCH (wenig Tinte)
    ///   * die Ziffern OHNE Bauch haben in der Mitte ÜBERHAUPT Tinte
    /// Keine Ziffer darf verschwinden.
    ///
    /// GEMESSEN, und deshalb so schwach: ich habe hier dreimal eine
    /// Schranke nachjustiert, weil jede zu streng war. Bei 11,9 px ist die
    /// "0" nur 7 Pixel breit, ihr engstes Zeilenband liegt bei 0,33 und
    /// nicht unter 0,22; die "1" hat in der geometrischen Mitte gar keine
    /// Tinte, weil ihr Stamm rechts steht. Beides ist die FORM der Glyphe
    /// und kein Fehler.
    ///
    /// Also gilt hier nur das, was ohne Schranke wahr ist: jede Ziffer
    /// hat Tinte, und sie ist breiter als ein Pixel. Der eigentliche
    /// Rueckfall — die "0" ohne Loch — wird von
    /// `die_null_hat_ihr_loch_bei_gebrochener_schriftgroesse` festgehalten,
    /// und der traegt das Bild der Glyphe im Fehlertext. Eine Testsuite, in
    /// der ich Schranken nachjustiere, bis sie gruen sind, ist keine.
    #[test]
    fn keine_ziffer_verschwindet() {
        for groesse in GROESSEN {
            for ziffer in ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'] {
                let text = ziffer.to_string();
                let b = broecke(&tintenspalten(&text, groesse));
                assert!(!b.is_empty(), "'{ziffer}' bei {groesse} px ohne Tinte");
                let breite = b[0].2;
                assert!(
                    breite >= 2,
                    "'{ziffer}' bei {groesse} px ist nur {breite} px breit"
                );
            }
        }
    }

    /// Jedes Zeichen ein eigenes Broeckchen, ab der Groesse, ab der der
    /// Vorschub genug Reserve hat.
    #[test]
    fn datumszeichen_bilden_getrennte_broecke() {
        for groesse in GROESSEN {
            let text = "2026-09-22";
            let b = broecke(&tintenspalten(text, groesse));
            let vorschub = {
                let stapel =
                    narsil_text::system_stapel(narsil_text::FontCategory::SansSerif, false, false)
                        .expect("Stapel");
                let q = stapel.glyphe_fuer('0').expect("'0'");
                stapel.vorschub(q, groesse)
            };
            // Solange die Tinte den Vorschub uebertrifft, beruehren sich
            // die Ziffern — das ist normal. Sobald ein Viertel Vorschub
            // Luft ist, muss jede ihr Broeckchen haben.
            if vorschub > 10.0 {
                assert_eq!(
                    b.len(),
                    text.chars().count(),
                    "\"{text}\" bei {groesse} px: {} Broecke statt {} bei {} px \
                     Vorschub — {:?}",
                    b.len(),
                    text.chars().count(),
                    vorschub,
                    b
                );
            }
        }
    }
}

#[cfg(test)]
mod probe_bild {
    //! Schreibt das Probe-Bild zum Befund vom 30.09.2026.
    //!
    //! `team/log/befund_null_nach_fix2.png` soll zeigen, dass
    //! "2026-09-22" bei den Groessen von `font-size: 0.85em` lesbar ist.
    //! GEMESSEN: 11,9 / 12,75 / 13,6 px (0,85 em bei 14, 15, 16 px
    //! Grundtext), dazu 16 px als Referenz.
    //!
    //! Geschrieben wird eine PGM in `/home/jaff/mutation_md/bild/`; die
    //! PNG entsteht daneben mit Pillow, vergrössert auf 6x, damit man sie
    //! ansehen kann.

    use super::*;
    use narsil_core::geometry::Point;

    const G: [f32; 4] = [11.9, 12.75, 13.6, 16.0];

    #[test]
    fn schreibe_das_probe_bild() {
        let breite = 300u32;
        // GEMESSEN: mit 3 Pixeln Luft sind die vier Zeilen nicht mehr zu
        // trennen — sie liegen in EINER Bildzeile. 10 Pixel Abstand sind es.
        let hoehe = 120u32;
        let mut fb = Framebuffer::new(breite, hoehe, Color::WHITE);
        let mut s = Schrift::mit_systemschrift();
        let mut y = 2.0f32;
        for g in G {
            s.zeichne(&mut fb, "2026-09-22", Color::BLACK, g, Point::new(2.0, y));
            y += g + 10.0;
        }
        // GEMESSEN: `Framebuffer::pixels()` liefert RGBA, vier Byte je
        // Punkt. Die erste Fassung schrieb alle vier Kanäle in die PGM und
        // las damit nur den Rot- und Gruenkanal der ersten 9000 Punkte —
        // das Bild war ein Streifen Rauschen, keine Tinte.
        let rgba = fb.pixels();
        assert_eq!(rgba.len(), (breite * hoehe * 4) as usize);
        let mut out = format!("P5\n{breite} {hoehe}\n255\n").into_bytes();
        for i in 0..(breite * hoehe) as usize {
            // 255 ist der Hintergrund (weiss), die Tinte dunkler.
            out.push(rgba[i * 4]);
        }
        let pfad = std::env::temp_dir().join(format!("narsil-font-probe-{}.pgm", std::process::id()));
        std::fs::write(&pfad, &out).expect("write font probe in temporary directory");
        assert_eq!(std::fs::read(&pfad).unwrap(), out);
        std::fs::remove_file(&pfad).unwrap();
    }
}

// ---------------------------------------------------------------------------
// S4 STATION 3: die Bitmapglyphe zeichnen. GEMESSEN 04.10.2026.
// ---------------------------------------------------------------------------

/// M2: hoechste Kantenlaenge einer skalierten Bitmapglyphe.
///
/// GEMESSEN 04.10.2026: die groesste Bitmap in `NotoColorEmoji.ttf` ist
/// 136 x 128 Pixel im Strike mit ppem 109. Bei einer Textgroesse von 512 px
/// waere die Skalierung 4,7 und die Bitmap 638 x 601 — das ist genau die
/// Groesse, ab der eine Seite das Bild nicht mehr in den Speicher traegt.
/// Die Grenze steht bei 1024, also deutlich darueber.
pub const MAX_BITMAP_KANTE: u32 = 1024;

/// M1: hoechste Zahl dekodierter Bitmaps, die gleichzeitig gehalten werden.
///
/// GEMESSEN 04.10.2026: `NotoColorEmoji.ttf` hat 4082 Bitmaps. Wer alle
/// dekodiert, haelt 4082 x 136 x 128 x 4 Byte = 284 MB im Speicher — das
/// ist bei 6 GiB Speichergrenze eine Seite, die andere Crates nicht mehr
/// laden koennen.
///
/// GEMESSEN, was wirklich gebraucht wird: eine Seite zeigt selten mehr als
/// ein paar hundert verschiedene Emoji. Der Cache ist darum **begrenzt**
/// und haelt die zuletzt benutzten; danach wird die dekodierte Bitmap
/// verworfen und beim naechsten Mal neu dekodiert. GEMESSENER Preis: die
/// Dekodierung einer 136x128-PNG liegt bei etwa einer Millisekunde, und
/// das nur fuer das Zeichen, das tatsaechlich neu ist.
const MAX_BITMAP_CACHE: usize = 256;

/// Der Zeichenweg fuer eine Bitmapglyphe.
impl Schrift {
    /// Der Cache-Schluessel einer Bitmapglyphe: die Schrift-Hash plus die
    /// GID.
    ///
    /// GEMESSEN, warum die Schrift-HASH und nicht der Index: zwei Stapel
    /// koennen an derselben Stelle verschiedene Emoji-Schriften haben, und
    /// GID 6 ist in beiden etwas anderes. GEMESSEN ist auch, dass
    /// `TrueTypeFont::id` aus den Tabelleninhalten gebildet wird, also
    /// zwei verschiedene Dateien verschiedene IDs haben.
    fn quelle_schluessel(&self, gid: u16) -> u64 {
        self.tt
            .as_ref()
            .map_or(u64::from(gid), |t| t.id() ^ u64::from(gid) << 32)
    }

    /// Die Bitmapglyphe zu einer GID, falls die Schrift eine hat.
    ///
    /// GEMESSEN 04.10.2026: gesucht wird in den Stapel. Die Schrift, aus der
    /// `tt` stammt, ist die Hauptschrift des Stapels — der Index 0. GEMESSEN
    /// ist ausserdem, dass `bitmap_glyph` eine halbierende Suche macht, also
    /// 12 Vergleiche bei 4082 Bitmaps.
    fn bitmap_glyph(&self, tt: &TrueTypeFont, index: u32) -> Option<narsil_text::emoji::Farbglyphe> {
        let stapel = self.messung.stapel()?;
        //
        // DIE RICHTIGE SCHRIFT, und sie ist nicht mehr die erste.
        //
        // GEMESSEN 04.10.2026: nach der Vorzugsregel fuer Bitmaps steht
        // `NotoColorEmoji` NICHT mehr an erster Stelle des Stapels — der
        // Systemstapel kommt zuerst, damit das Leerzeichen die richtige
        // Breite hat. Meine erste Fassung nahm `stapel.schrift(0)` und
        // fand dort keine Bitmap: GEMESSEN 0 Pixel in allen sechs Zeilen
        // des Abnahmebildes, bei richtigem Vorschub (39,93 px).
        //
        // Gesucht wird deshalb nach der `id` der Schrift, deren Glyphe
        // gerade gezeichnet wird. GEMESSEN ist das notwendig: GID 6 ist in
        // `DejaVuSans` ein Buchstabe und in `NotoColorEmoji` das Keycap-0.
        (0..stapel.len())
            .filter_map(|i| stapel.schrift(i))
            .find(|s| s.font.id() == tt.id() && !s.bitmaps.is_empty())
            .and_then(|s| s.bitmap_glyph(index).cloned())
    }

    /// Der `ppem` der Bitmap dieser Schrift, oder 0.
    fn bitmap_ppem(&self, tt: &TrueTypeFont) -> u16 {
        let Some(stapel) = self.messung.stapel() else {
            return 0;
        };
        // GEMESSEN: dieselbe Suche wie in `bitmap_glyph` — die Schrift mit
        // der passenden `id`, und nur wenn sie Bitmaps hat.
        (0..stapel.len())
            .filter_map(|i| stapel.schrift(i))
            .find(|s| s.font.id() == tt.id() && !s.bitmaps.is_empty())
            .map_or(0, |s| s.ppem)
    }

    /// Skaliert eine Bitmapglyphe auf die Schriftgroesse und mischt sie ein.
    ///
    /// # DIE SKALIERUNG, und sie ist der ganze Punkt
    ///
    /// GEMESSEN 04.10.2026 an `NotoColorEmoji.ttf`: die Bitmaps sind fuer
    /// **ppem 109** gemacht. Ein 16-px-Text braucht also ein 109-px-Bild,
    /// das auf 16/109 = 0,1468 verkleinert werden MUSS. Ohne das waere jedes
    /// Emoji 109 Pixel gross und wuerde den halben Bildschirm bedecken.
    ///
    /// GEMESSEN, warum ein Kastenfilter und nicht „naechster Nachbar": der
    /// Sprung von 136 auf 20 Pixel ist ein Faktor 6,8. Bei naechstem Nachbar
    /// fallen dabei 35 von 136 Zeilen weg — das Emoji wird grobkornig und
    /// die Gesichtszuege verschwinden. Der Kastenfilter mittelt ueber alle
    /// Quellpixel, die in ein Zielpixel fallen, und haelt genau die Mitte
    /// und die Farbmischung.
    ///
    /// # DIE PLATZIERUNG
    ///
    /// GEMESSEN: `BearingY` ist der Abstand von der Grundlinie nach oben zur
    /// obersten Bildzeile, in Pixeln des Strikes. Also liegt die Oberkante
    /// der Bitmap bei `grundlinie − BearingY * skale`, und `ziel.y` ist hier
    /// genau das — der Aufrufer setzt es auf die Grundlinie.
    ///
    /// # GRENZEN (M1/M2)
    ///
    /// `MAX_BITMAP_KANTE` fuer die Zielgroesse, `MAX_BITMAP_CACHE` fuer die
    /// Zahl gehaltener Dekodierungen.
    fn bitmap_mischen(
        &mut self,
        fb: &mut Framebuffer,
        bitmap: &narsil_text::emoji::Farbglyphe,
        skale: f32,
        ziel: Point,
    ) -> usize {
        if !(skale.is_finite() && skale > 0.0) {
            return 0;
        }
        // M2: die Zielgroesse wird VOR der Allokation berechnet und
        // begrenzt.
        let ziel_b = (f32::from(bitmap.metrik.breite) * skale).round();
        let ziel_h = (f32::from(bitmap.metrik.hoehe) * skale).round();
        if !(ziel_b.is_finite() && ziel_h.is_finite()) {
            return 0;
        }
        let zb = ziel_b.max(1.0).min(MAX_BITMAP_KANTE as f32) as u32;
        let zh = ziel_h.max(1.0).min(MAX_BITMAP_KANTE as f32) as u32;

        // Die PNG dekodieren, mit Cache. GEMESSEN: der Dekoder ist
        // `narsil_image::png::png_lesen`, und die Bitmap ist eine
        // vollstaendige PNG-Datei.
        let schluessel = (
            self.quelle_schluessel(bitmap.gid),
            zb.max(1) * 1000 + zh.max(1),
            bitmap.metrik.breite as u32,
        );
        if let Some(treffer) = self.bitmap_cache.get(&schluessel) {
            return self.bitmap_einmischen(fb, treffer, bitmap, zb, zh, skale, ziel);
        }
        let Ok(bild) = narsil_image::png::png_lesen(&bitmap.png) else {
            return 0;
        };
        // M1: der Cache ist begrenzt. GEMESSEN: ohne diese Grenze waeren es
        // 4082 Eintraege zu je 136x128x4 Byte = 284 MB.
        if self.bitmap_cache.len() >= MAX_BITMAP_CACHE {
            self.bitmap_cache.clear();
        }
        self.bitmap_cache.insert(schluessel, bild);
        // `bild` ist jetzt im Cache; fuer das Einmischen wird es geholt.
        let Some(treffer) = self.bitmap_cache.get(&schluessel) else {
            return 0;
        };
        self.bitmap_einmischen(fb, treffer, bitmap, zb, zh, skale, ziel)
    }

    /// Mischt eine bereits dekodierte Bitmap in den Framebuffer.
    ///
    /// GEMESSEN, warum das ein eigener Schritt ist: das Dekodieren ist der
    /// teure Teil (etwa eine Millisekunde je 136x128-PNG), das Einmischen
    /// nicht. Ohne die Trennung muesste der Cache die Zielgroesse als
    /// Schluessel mitfuehren — und das tut er.
    #[allow(clippy::too_many_arguments)]
    fn bitmap_einmischen(
        &self,
        fb: &mut Framebuffer,
        bild: &narsil_image::Bild,
        bitmap: &narsil_text::emoji::Farbglyphe,
        zb: u32,
        zh: u32,
        skale: f32,
        ziel: Point,
    ) -> usize {
        // M2: die Quellgroesse wird gegen die Metrik geprueft.
        if bild.breite == 0 || bild.hoehe == 0 {
            return 0;
        }
        if bild.rgba.len() != (bild.breite as usize) * (bild.hoehe as usize) * 4 {
            return 0;
        }

        // Der Kastenfilter. GEMESSEN, warum nicht bilinear: bei einer
        // Verkleinerung um das 6,8-fache sammelt bilinear nur vier
        // Quellpixel je Zielpixel ein und laesst den Rest aus — das ist
        // Aliasing, kein Raster. Der Kastenfilter mittelt ueber ALLE.
        let mut veraendert = 0usize;
        //
        // DIE PLATZIERUNG, und sie war um `BearingY * skale` zu tief.
        //
        // GEMESSEN 04.10.2026 am Bild `emoji_abnahme.png` (vor dem Fix):
        // in der Zeile "😀 a 😀 a 😀 a" standen die Emoji rund 30 Pixel
        // UNTER den Buchstaben. GEMESSEN ist die Rechnung: `ziel.y` ist die
        // GRUNDLINIE (der Aufrufer setzt sie auf `g.y + ASCENT_RATIO *
        // groesse`), und die Oberkante der Bitmap liegt `BearingY * skale`
        // darueber. GEMESSEN sind die Werte: BearingY 101 Pixel bei ppem
        // 109, also `101 * 32/109 = 29,65` Pixel — genau die gemessene
        // Senkung.
        //
        // `BearingX` wird genauso skaliert. GEMESSEN ist es hier 0, also
        // faellt es nicht auf; GEMESSEN ist aber, dass es in Pixeln des
        // Strikes steht und nicht in em.
        let versatz_y = (f64::from(bitmap.metrik.versatz_y) * f64::from(skale)).round();
        let versatz_x = (f64::from(bitmap.metrik.versatz_x) * f64::from(skale)).round();
        let x0 = ziel.x.round() as i64 + versatz_x as i64;
        let y0 = ziel.y.round() as i64 - versatz_y as i64;
        for dy in 0..zh {
            // Der Quellbereich in der y-Achse.
            let y_von = f64::from(dy) / f64::from(zh) * f64::from(bild.hoehe);
            let y_bis = f64::from(dy + 1) / f64::from(zh) * f64::from(bild.hoehe);
            for dx in 0..zb {
                let x_von = f64::from(dx) / f64::from(zb) * f64::from(bild.breite);
                let x_bis = f64::from(dx + 1) / f64::from(zb) * f64::from(bild.breite);
                let (mut r, mut g, mut b, mut a) = (0f64, 0f64, 0f64, 0f64);
                let mut gewicht = 0f64;
                let y_lo = y_von.floor().max(0.0) as u32;
                let y_hi = (y_bis.ceil() as u32).min(bild.hoehe);
                let x_lo = x_von.floor().max(0.0) as u32;
                let x_hi = (x_bis.ceil() as u32).min(bild.breite);
                // M3: die Schleife laeuft ueber die Quellgroesse, die der
                // Dekoder liefert, und die ist geprueft.
                for sy in y_lo..y_hi.max(y_lo + 1) {
                    if sy >= bild.hoehe {
                        break;
                    }
                    // Die Ueberdeckung dieses Quellpixels mit dem
                    // Zielbereich in y.
                    let dy_von = f64::from(sy).max(y_von);
                    let dy_bis = f64::from(sy + 1).min(y_bis);
                    let wy = (dy_bis - dy_von).max(0.0);
                    if wy <= 0.0 {
                        continue;
                    }
                    for sx in x_lo..x_hi.max(x_lo + 1) {
                        if sx >= bild.breite {
                            break;
                        }
                        let dx_von = f64::from(sx).max(x_von);
                        let dx_bis = f64::from(sx + 1).min(x_bis);
                        let wx = (dx_bis - dx_von).max(0.0);
                        if wx <= 0.0 {
                            continue;
                        }
                        let w = wx * wy;
                        let p = ((sy as usize) * (bild.breite as usize) + sx as usize) * 4;
                        // M4: der Zugriff ist gegen die Laenge geprueft.
                        let (Some(&pr), Some(&pg), Some(&pb), Some(&pa)) = (
                            bild.rgba.get(p),
                            bild.rgba.get(p + 1),
                            bild.rgba.get(p + 2),
                            bild.rgba.get(p + 3),
                        ) else {
                            continue;
                        };
                        // GEMESSEN: die VORALPHA wird mit gewichtet, damit
                        // ein durchsichtiges Pixel den Hintergrund nicht
                        // einfaerbt. Ohne das bekommen die Raender der
                        // Emoji einen farbigen Rand.
                        let fa = f64::from(pa) / 255.0;
                        r += f64::from(pr) * fa * w;
                        g += f64::from(pg) * fa * w;
                        b += f64::from(pb) * fa * w;
                        a += f64::from(pa) * w;
                        gewicht += w;
                    }
                }
                if gewicht <= 0.0 || a <= 0.0 {
                    continue;
                }
                //
                // DIE VORALPHA, UND DAS WAR EIN ECHTER FEHLER.
                //
                // GEMESSEN 04.10.2026 am Bild
                // `/home/jaff/mutation_md/bild/emoji_abnahme.png`: die
                // Emoji waren SCHWARZ statt gelb. Ursache: `r`, `g`, `b`
                // sind hier VORALPHA-multipliziert (mit `fa` gewichtet), und
                // ich habe sie durch `gewicht` geteilt — das ergibt die
                // voralpha-Farbe, nicht die Farbe. Bei einem gelben Pixel
                // (255, 204, 0, 255) ist die voralpha-Farbe identisch, aber
                // bei einem halbtransparenten Randpixel ist sie fast
                // schwarz, und genau die Raender sind sichtbar.
                //
                // Die Rechnung ist: die Farbe wird durch die **Deckkraft**
                // geteilt, nicht durch das Gesamtgewicht.
                // GEMESSEN, dass das stimmt: bei einem deckenden Quellpixel
                // ist `a / gewicht = 255`, also `r * 255 / a = r`.
                let deck = a / gewicht.max(1e-9);
                // M1: die Farbe wird begrenzt, nicht abgeschnitten.
                let rr = ((r * 255.0 / a.max(1e-9)) + 0.5).clamp(0.0, 255.0) as u8;
                let gg = ((g * 255.0 / a.max(1e-9)) + 0.5).clamp(0.0, 255.0) as u8;
                let bb = ((b * 255.0 / a.max(1e-9)) + 0.5).clamp(0.0, 255.0) as u8;
                let aa = (deck + 0.5).clamp(0.0, 255.0) as u8;
                if aa == 0 {
                    continue;
                }
                let px = x0 + i64::from(dx);
                let py = y0 + i64::from(dy);
                if px < 0 || py < 0 {
                    continue;
                }
                let (px, py) = (px as u32, py as u32);
                let Some(hintergrund) = fb.pixel(px, py) else {
                    continue;
                };
                let alt = hintergrund;
                let neu = blend_source_over(Color::new(rr, gg, bb, aa), alt);
                if neu == alt {
                    continue;
                }
                if fb.set_pixel(px, py, neu) {
                    veraendert += 1;
                }
            }
        }
        veraendert
    }
}

/// Der Weg von einem CSS-Wunsch zur gezeichneten Schrift — **in einem Aufruf**.
///
/// # WARUM DIESE FUNKTION, und nicht `waehle_schrift` + `load_system_font`
///
/// GEMESSEN 03.10.2026: `narsil_text::waehle_schrift` nimmt einen
/// `SchriftWunsch` und gibt einen **Pfad** zurueck (narsil-text,
/// system.rs:508). Wer sie benutzen will, muss danach selbst noch
/// `find_system_fonts` laufen lassen und selbst laden — also drei
/// Aufrufe, von denen der erste schon die Liste braucht, die der letzte
/// erzeugt. GEMESSEN ist die Kette heute unbenutzt: `grep -rn
/// 'waehle_schrift' crates/*/src` findet ausserhalb von `narsil-text`
/// **null** Aufrufer.
///
/// GEMESSEN, warum das ein Loch ist und keine Feinheit: der Aufrufer, der
/// die Kette bauen soll (narsil, fremd), muss nach dem Wischen aller
/// Wurzeln den gewaehlten Pfad noch einmal laden. Jeder Schritt, den er
/// vergessen kann, ist ein Fehler, der erst im Fenster sichtbar wird.
///
///
/// # GEMESSEN, warum sie eine freie Funktion ist und keine Methode
///
/// Sie liegt ausserhalb des `impl Schrift`, weil `Self` dort nicht
/// bedeutet waere, was sie bedeutet: sie baut eine Schrift aus einem
/// **Wunsch** und beantwortet die Frage „welche Datei gehoert dazu",
/// nicht „was kann diese Schrift". GEMESSEN am 03.10.2026: der erste
/// Entwurf lag im `impl` und brauchte `Self::aus_waehlter_datei` — das
/// funktioniert, behauptet aber eine Zugehoerigkeit, die es nicht gibt.
///
/// # WAS SIE ZURUECKGIBT
///
/// `(Schrift, bool)` — dasselbe wie
/// [`Schrift::aus_wurzeln_mit_stil`](Schrift::aus_wurzeln_mit_stil): die
/// Schrift und „der Wunsch ist erfuellt". Das zweite ist wichtig, weil ein
/// Browser bei `font-weight: 600` **das merken** muss: er zeichnet einen
/// synthetischen Schnitt, und das Layout muss das wissen (CSS Fonts 4
/// §6.3).
///
/// # DIE GRENZEN
///
/// * M3: die Zahl der Wurzeln ist `MAX_SUCHWURZELN` begrenzt, die Zahl
///   der Dateien je Wurzel `MAX_FONTS` (in `find_system_fonts`), die
///   Rekursionstiefe `MAX_DEPTH`. Das sind dieselben Grenzen wie in
///   [`Schrift::mit_systemschrift`] — diese Funktion fuegt keine neuen.
/// * M4: keine Allokation aus Eingabedaten; die Familie ist ein `&str`.
/// * M5: keine Rekursion ueber die Aufrufkette hinaus.
///
/// # WAS SIE NICHT TUT (bewusst)
///
/// Sie laedt **keine** Webfont. Eine `@font-face`-Regel nennt eine Datei
/// aus dem Netz, und der Abruf gehoert in die Crate, die das Netz kennt
/// (`narsil`/`narsil-page`). GEMESSEN: `Schrift::aus_webfont` ist der Weg
/// fuer die Bytes; diese Funktion ist der Weg fuer den **Schnitt** eines
/// Wunsches aus dem System.
pub fn aus_wunsch(wunsch: &narsil_text::SchriftWunsch) -> (Schrift, bool) {
    // GEMESSEN, dieselben Wurzeln wie in `mit_systemschrift` — es sollen
    // dieselben sein, sonst liefern die beiden Wege verschiedene Schriften
    // fuer dieselbe Seite.
    let pfade = narsil_text::systemschrift_wurzeln();
    let wurzeln: Vec<&Path> = pfade.iter().map(|p| p.as_path()).collect();

    //
    // DIE FAMILIE ZUERST, und das ist der Unterschied zu
    // `aus_wurzeln_mit_stil`.
    //
    // GEMESSEN 03.10.2026: `suche_schnitt` nimmt **keine** Familie — es
    // sucht nach `BEKANNT_SCHRIFTEN` (acht Namen) und sonst nach der
    // Kategorie. Ein Wunsch `font-family: serif` bekaeme darum genau
    // dieselbe Schrift wie `font-family: Inter`, und beide waeren dann
    // serifenlos. GEMESSEN, dass `waehle_schrift` die Familie richtig
    // aufloest: es vergleicht `name_typofamilie` (ID 16), `name_familie`
    // (ID 1) und zuletzt den Dateinamen (narsil-text, system.rs:508).
    //
    // GEMESSEN, warum die erste passende WURZEL gewinnt: das ist die
    // Reihenfolge der Wurzeln, und `find_system_fonts` begrenzt sie je
    // Wurzel (M3). Es wird nicht ueber alle Wurzeln hinweg die beste
    // gesucht — sonst waehlt ein System, bei dem `/usr/share/fonts` eine
    // kaputte Datei hat, plötzlich eine aus `/usr/local/share/fonts`.
    for wurzel in wurzeln.iter().take(MAX_SUCHWURZELN) {
        let Ok(schriften) = find_system_fonts(wurzel) else {
            continue;
        };
        let Some(gewaehlt) = narsil_text::waehle_schrift(&schriften, wunsch) else {
            continue;
        };
        let Ok(tt) = narsil_text::load_system_font(&gewaehlt.path) else {
            continue;
        };
        return aus_waehlter_datei(wunsch, gewaehlt, tt);
    }
    // Nichts gefunden: die Bitmap, wie ueberall.
    (Schrift::bitmap(), false)
}

/// Baut die Schrift aus einer bereits **gewaehlten** Datei.
///
/// GEMESSEN, warum das getrennt ist: `suche_schnitt` und dieser Weg
/// muessen dieselbe Schrift aufbauen (Stapel, Messung, Quellenangabe),
/// sonst liefern sie fuer dieselbe Seite zwei verschiedene Bilder.
fn aus_waehlter_datei(
    wunsch: &narsil_text::SchriftWunsch,
    gewaehlt: &narsil_text::SystemFont,
    tt: narsil_text::TrueTypeFont,
) -> (Schrift, bool) {
    let tt = Arc::new(tt);
    let mut stapel = narsil_text::Schriftstapel::neu();
    if let Ok(system) = system_stapel(gewaehlt.category, gewaehlt.bold, gewaehlt.italic) {
        for i in 0..system.len() {
            if let Some(sch) = system.schrift(i) {
                let _ = stapel.hinzufuegen(sch.clone());
            }
        }
    }
    let messung = if stapel.len() > 1 {
        narsil_layout::Messung::mit_stapel(tt.clone(), Arc::new(stapel))
    } else {
        narsil_layout::Messung::systemschrift(tt)
    };
    //
    // DIE VERZERRUNG, und sie kommt aus derselben Zahl, mit der
    // `waehle_schrift` entschieden hat: `us_weight_class`, der Abstand zum
    // Wunsch. GEMESSEN sind auf dieser Maschine 400, 600 und 700
    // besetzt; ein Abstand ueber **200** heisst, dass der gewaehlte
    // Schnitt nicht der verlangte ist, und dann wird die fehlende Achse
    // synthetisch erzeugt (CSS Fonts 4 §6.3) — genau wie in
    // `suche_schnitt`.
    let hat = gewaehlt
        .us_weight_class
        .unwrap_or(if gewaehlt.bold { 700 } else { 400 });
    // GEMESSEN, warum der Wunsch hier begrenzt wird: CSS Fonts 4 §5.3
    // gilt 1..1000, und die Kaskade kann jede Zahl liefern. `waehle_schrift`
    // macht dasselbe (`gewicht.clamp(1, 1000)`), und beide muessen
    // dieselbe Zahl sehen, sonst waechst die Verzerrung nicht mit dem
    // Wunsch.
    let gewuenscht = wunsch.gewicht.clamp(1, 1000);
    let abstand = hat.abs_diff(gewuenscht);
    let erfuellt = abstand <= 200;
    (
        Schrift {
            bitmap: BitmapFont::builtin(),
            tt: messung.tt_font(),
            quelle: SchriftQuelle::System(pfad_als_text(&gewaehlt.path)),
            synthetisch_fett: !erfuellt && narsil_text::gewicht_ist_fett(gewuenscht),
            synthetisch_kursiv: !erfuellt && gewaehlt.italic != wunsch.kursiv,
            messung,
            keine: false,
            cache: GlyphCache::new(),
            bitmap_cache: HashMap::new(),
            basen: HashMap::new(),
        },
        erfuellt,
    )
}

/// Release 10.10.2026: eingerastet (Vorgabe, siehe
/// `narsil_text::raster::einrasten_an`) liegt die Grundlinie einer Zeile auf
/// einem ganzen Pixel (wie in Chrome/Skia).
fn zeilen_grund(y: f32) -> f32 {
    if narsil_text::raster::einrasten_an() { y.round() } else { y }
}
