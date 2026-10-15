//! narsil-css — CSS-Teilmenge: Parser, Selektoren, Kaskade.
//!
//! # WAS DIESE CRATE IST
//!
//! Sie liest CSS-Quelltext (aus `<style>`-Bloecken, `.css`-Dateien oder
//! `style=""`-Attributen) und beantwortet EINE Frage je Element: "welche
//! berechneten Werte gelten hier?" Dafuer gibt es drei Schritte:
//!
//! 1. [`stylesheet_lesen`] / [`inline_lesen`] parsen Text zu
//!    [`Stylesheet`] / [`Deklaration`] (Fehlererholung: kaputte Regeln
//!    verwerfen nur sich selbst).
//! 2. [`berechnen`] loest fuer ein [`Element`] die Kaskade auf
//!    (Spezifitaet, Reihenfolge, `!important`, `@media`, Vererbung).
//! 3. Das Ergebnis ist [`Berechnet`] — konkrete Werte (px, Farben),
//!    mit denen die Rollen HTML und spaeter Layout arbeiten.
//!
//! Die oeffentliche Schnittstelle ist zusaetzlich in
//! `team/VERTRAG_CSS.txt` festgeschrieben (Auftrag C0, Welle 4).
//!
//! # SPEICHERREGELN (Welle 4, M3/M4)
//!
//! Jede Schleife, die von der Eingabe abhaengt, endet an einer der
//! `MAX_*`-Konstanten unten; jede Allokation wird VOR dem Anlegen
//! gedeckelt (`min(eingabe, GRENZE)`, niemals `with_capacity(eingabe)`).
//! `unsafe` gibt es nicht, `todo!` gibt es nicht, Panic bei Eingaben
//! gibt es nicht: was nicht passt, wird verworfen und gezaehlt
//! ([`Stylesheet::verworfen`]).

pub mod berechnet_css;
pub mod bewegung;
// Release 12.10.2026: Werte-CSSOM (Grammatik je Eigenschaft, kanonische
// Schreibweise, Kurzformen, Deklarationsblock) — siehe `cssom/mod.rs`.
pub mod cssom;
mod boxmodell;
mod farben;
mod flex;
mod form_state;
// Release 11.10.2026: die Rasterwerte des neuen Rasteralgorithmus.
pub mod gitter;
mod kaskade;
pub mod kosmetik;
mod layout_plan;
mod parse;
mod position;
mod raster;
mod rechnen;
mod attr_kaskade;
mod schicht;
mod scope;
mod scope_match;
pub use scope_match::matches_with_proximity;
/// Release 08.10.2026: `var()` aufloesen (fuer CSS-Werte, die als Attribute
/// weitergehen, z. B. `fill` an Inline-SVG).
pub use rechnen::{ersetze_inherit, ersetze_var};
// Release 12.10.2026 (Agent CSS2): `calc()` fuer Werte, die erst der Seitenbau
// rechnet (`vertical-align` gegen die Zeilenhoehe).
pub use rechnen::{RechenCtx, rechne};
mod supports;
mod token;
mod transform;
mod typen;
mod ua;

pub use boxmodell::{
    BlockErgebnis, BlockVorgabe, anteil, kollaps, lege_block_breite, lege_block_hoehe,
};
pub use flex::{
    FlexKind, FlexStreifen, FlexZeile, KindQuer, MAX_FLEX_KINDER, lege_flex, lege_flex_zeilen,
};
pub use kaskade::schrift_masse_anmelden;
pub use kaskade::{
    Vormerker, berechnen, berechnen_mit, berechnen_mit_vormerker, berechnen_voll, profil_lesen,
    profil_setzen,
};
// Der Weg von der Kaskade ins Layout (Rolle LAYOUT, W7 L1–L4).
// P19 (05.10.2026, 20:39): `layout_grenzen_prozent` ist der Weg, auf dem
// `max-width: 100%` den Motor erreicht, ohne dass die Bruecke die Zahl
// der Kaskade nehmen muss (die ist gegen die ANSICHT gerechnet).
pub use layout_plan::{
    MAX_WERT_PX, erzeugt_stapelkontext, layout_grenzen, layout_grenzen_prozent, layout_plan_setze,
    layout_stil,
};
pub use parse::{media_trifft, restrict_to_media};
// Release 11.10.2026: fuer `<input type=color>` (narsil, HTML §4.10.5.1.15).
pub use parse::farbe_aus;
pub use parse::{
    inline_lesen, inline_variablen, selektorliste_lesen, stylesheet_lesen, stylesheet_lesen_gemerkt,
    wert_gueltig, zaehle, zaehler_lesen, zaehler_nullen,
};
pub use position::{
    Rechteck, Schicht, Vorfahr, absolut_senkrecht, absolut_waagerecht, beschnitt,
    beschnitt_ueberlauf, clear_y, enthaltender_block, float_x, mal_reihenfolge, relativ_versatz,
    ueberlauf_rollt, ueberlauf_verbirgt,
};
// L4 (02.10.2026 08:1x): `lege_raster_bei` mit getrennten
// Achsluecken (CSS Grid 1 §10.1). `lege_raster` bleibt daneben — es ist
// der Stand mit EINEM Lueckenwert fuer beide Achsen, und die Tests
// in `narsil-css/tests/position.rs` behaupten genau den.
// L4 (02.10.2026 11:3x): `lege_raster_bei_fluss` dazu —
// `grid-auto-flow: column` braucht eine eigene Plazierung, und die
// alte Form bleibt als Huelle fuer die Aufrufstellen, die den
// Anfangswert `row` behaupten.
// L4 (02.10.2026 19:4x): `lege_raster_bei_zeilen` dazu —
// `grid-template-rows` (CSS Grid 1 §8.3) rechnet die Zeilenhoehe. Die
// beiden aelteren Namen bleiben als HUELLEN und rechnen ohne
// Zeilenspuren, also mit dem alten Verhalten; sieben Aufrufstellen in
// `tests/position.rs` behaupten es.
pub use raster::{
    RasterKind, RasterZelle, lege_raster, lege_raster_bei, lege_raster_bei_autom,
    lege_raster_bei_dicht, lege_raster_bei_fluss, lege_raster_bei_gebiete, lege_raster_bei_zeilen,
    lege_raster_spuren,
};
pub use supports::supports_trifft;
pub use typen::{
    Anzeige, AttrBed, AttrOp, Berechnet, Bidi, BildGeo, BildQuelle, BorderImage, BorderImageWidth, BoxArt, Deklaration, Eigenschaft,
    Einfach, Einheit, Farbe, Farbschema, Farbstopp, FlexAusrichtung, FlexBuendig, FlexRichtung,
    Float, Gebiet, KettenGlied, Kombinator, Laenge, Liste, MaskGroesse, Maske, MediaAbfrage,
    MediaUnd, MischModus, Position, Pseudo, Raeumung, Rahmenstil, Regel, Schatten, SchreibRichtung,
    SchriftQuelle, Schriftstil, Selektor, SpaltenMass, StilRegel, Stylesheet, Textausrichtung, ZeilenhoeheArt,
    Ueberlauf, Variablen, VariablenDekl, Verlauf, VerlaufArt, Weissraum, Wert, Zustand,
    selektor_passt, selektor_passt_mit, wird_vererbt,
};
pub use ua::{UA_CSS, ua_stylesheet};

/// Anzahl unterstuetzter Farbnamen.
///
/// K7c (01.10.2026): 151 — die 148 BENANNTEN Farben aus CSS Color 4
/// §6.1 plus drei Systemfarben aus §6.2 (`graytext`, `canvas`,
/// `canvastext`), deren RGB-Werte in `farben.rs` FESTGELEGT sind, weil
/// sie keine browseruebergreifenden haben. Der Test sichert die Zahl
/// zu (M7).
pub fn farbnamen_anzahl() -> usize {
    farben::anzahl()
}

/// Trait, das die Rolle HTML fuer ihre Knoten implementiert, damit die
/// Kaskade Selektoren pruefen kann. Naeheres im Vertrag (Abschnitt 2).
pub use typen::Element;

// ===========================================================================
// GRENZEN — jede mit Begruendung (M3/M4). Wer mehr liefert, bekommt ein
// begrenztes Ergebnis statt eines Absturzes.
// ===========================================================================

/// Maximale Eingabelaenge: 4 MiB. YouTubes Haupt-CSS misst 3,54 MiB.
/// Die Grenze wird vor dem Tokenisieren angewendet.
pub const MAX_EINGABE: usize = 4 * 1024 * 1024;

/// Tokenbudget fuer grosse gebuendelte Stylesheets. Die bisherige
/// Grenze von 64.000 kappte Dateien weit unterhalb der Eingabegrenze.
/// Die Kaskade verwendet den vorhandenen Selektorindex; Token werden
/// beim Parsen freigegeben, statt fuer jedes Element neu erzeugt.
pub const MAX_TOKEN: usize = 1_000_000;

/// Release 11.10.2026 (Agent CSS2): trennt im Text eines Blattes zwei
/// Blaetter, die unabhaengig gelesen werden (ein per `@import` eingesetztes
/// Blatt und das einbindende): ein Lesefehler im einen (offene Klammer,
/// falsche Kodierung) verschluckt die Regeln des anderen nicht. Zwei
/// Zeichen aus dem privaten Bereich, die in echtem CSS nicht vorkommen.
pub const BLATT_GRENZE: &str = "\u{F8FF}narsil-blatt\u{F8FF}";

/// Maximale Regelzahl pro Stylesheet, auch fuer grosse Anwendungen.
///
/// Release 09.10.2026: GEMESSEN war 16 384 zu wenig — YouTubes Hauptblatt
/// (`ytmainappweb.kevlar_base`, 3,18 MiB) hat 20 994 Regeln; die letzten
/// ~4 600 (Komponentenstile) fielen still weg. 65 536 Regeln passen zur
/// Eingabegrenze von 4 MiB (eine Regel kostet mindestens `a{}`, also
/// liegt die Menge weiter durch `MAX_EINGABE` gedeckelt, M4).
pub const MAX_REGELN: usize = 65_536;

/// Maximale Stylesheets je `berechnen`-Aufruf.
///
/// Release 08.10.2026: GEMESSEN war 16 zu wenig — github.com laedt 27
/// Blaetter, und das 24. (`global-nav-bar`, die Kopfleiste) fiel still
/// weg; die Seite stand ohne Flex-Layout da. 128 deckt echte Seiten ab,
/// die Schleife bleibt begrenzt (M3).
pub const MAX_BLAETTER: usize = 128;

/// Maximale Selektoren je Regel (Gruppe mit Komma). Begruendung: Gruppen
/// mit mehr als 16 Selektoren kommen in echten Seiten nicht vor; der
/// Rest der Gruppe faellt weg, die Regel bleibt gueltig.
///
/// GEMESSENE ENTSCHEIDUNG, DIE ANHEBUNG GING ZURUECK (01.10.2026 21:25)
///
/// Ich habe diese Grenze auf 4096 gesetzt, weil ich in
/// `wikipedia_vector-2022.css` `:lang()`-Bloecke mit Selektoren ueber
/// 1024 Zeichen gemessen zu haben glaubte.
///
/// GEMESSEN, drei Laeufe `examples/abdeckung.rs`, Debug-Build:
///
/// ```text
/// vorher (1024)  8.1 / 8.4 / 8.0 ms  580 Regeln  24 verworfen
/// nachher (4096) 8.1 / 8.1 / 8.4 ms  580 Regeln  24 verworfen
/// ```
///
/// EIGENER FEHLER, eine Stunde nach dem Befund in `narsil-layout`: Ich hatte
/// die Zahlen mit vier Leerzeichen eingerueckt. rustdoc liest eine
/// Einrueckung ab vier Leerzeichen als **Rust-Code** und versucht zu
/// kompilieren — `vorher (1024) 8.1 / …` ist kein Rust, und der
/// Doctest schlug mit „missing `fn` or `struct`" an. Genau der Fehler,
/// den ich um 20:43 in `crates/narsil-layout/src/plan.rs:483` bei der
/// Rolle PROBE gemeldet und nicht angefasst hatte. Die Lehre brauchte
/// offenbar einen eigenen Anlass.
///
/// **Keine Aenderung, weder in den Regeln noch in der Zeit.** Und die
/// Messung selbst war falsch: die ueber 1024 Zeichen langen
/// Selektoren waren ein Artefakt eines Regex-Zaehlers (siehe
/// `MAX_SELEKTOREN_JE_REGEL` unten). Mit string-bewusstem Zaehlen ist
/// der laengste echte Selektor dieser Datei 240 Zeichen.
///
/// Also zurueckgenommen: eine vierfach hoehere Speichergrenze ohne
/// einen einzigen gemessenen Gewinn ist kein Gewinn, sondern eine
/// Rechnung fuer die Zukunft, die niemand bestellt hat.
///
/// Warum ich es trotzdem gemessen habe: eine Grenze, deren
/// Begruendung auf einer Zahl beruht, die sich als falsch erweist, ist
/// ein Grund zu messen — nicht ein Grund, sie anzuheben. Sie steht
/// jetzt auf 1024, und die Begruendung ist die gemessene.
pub const MAX_SELEKTOR_LAENGE: usize = 1024;

/// Maximale Selektoren je Regel (Gruppe mit Komma).
///
/// GEMESSENE GRUNDLAGE (01.10.2026, 21:35): die groesste echte
/// Selektorliste im Korpus hat **7** Selektoren
/// (`wikipedia_vector-2022.css`). 16 laesst also Luft fuer alles,
/// was gemessen wurde.
///
/// GEMESSENE ENTSCHEIDUNG, DIE ANHEBUNG GING ZURUECK: Ich habe diese
/// Grenze auf 256 gesetzt, weil ich eine Kommaliste mit **252**
/// Selektoren gemessen zu haben glaubte.
///
/// **Diese Zahl war falsch.** Sie kam aus einem Werkzeug, das Bloecke
/// mit `[^{}]*\{[^{}]*\}` suchte — und die Datei enthaelt
/// `@counter-style`-Bloecke mit einem `}` INNERHALB eines Strings
/// (`suffix:') '}`). Der Regex hat die Bloecke zerschnitten und
/// Rulemente erfunden, die es nicht gibt.
///
/// Mit einem string-bewussten Zaehler (Strings ueberspringen, Klammern
/// zaehlen) ist die Messung:
///
/// ```text
///     Top-Level-Bloecke in der Datei        673
///     Bloecke mit `:lang()` im Selektor      2
///     groesste Selektorliste eines Blocks    7
///     Summe `:lang()`-Varianten             14
/// ```
///
/// GEMESSENE KOSTEN der (zurueckgenommenen) Anhebung auf 256 waere
/// `Selektor` = 40 B mal 256 = 10 240 B je Regel im schlimmsten Fall
/// gegenueber 640 B — eine 16fache Speichervervielfachung fuer null
/// gewonnene Regeln.
///
/// Warum ich es trotzdem gemessen habe: die alte Begruendung lautete
/// „Gruppen mit mehr als 16 Selektoren kommen in echten Seiten nicht
/// vor", und genau das ist eine Behauptung, die man pruefen muss.
/// Sie hat sich als richtig erwiesen — aber jetzt WEISS man es, statt
/// es zu behaupten.
///
/// RELEASE 08.10.2026, GEMESSEN WIDERLEGT: tachyons (rust-lang.org) hat
/// `.border-box,a,article,…,header,html,input[type=email],…,ul
/// {box-sizing:border-box}` — **42** Selektoren in einer Liste. Mit 16
/// fielen `header`, `section`, `li` usw. still heraus und behielten
/// `content-box`: der Kopf von rust-lang.org war 1 176 statt 1 152 px
/// breit. normalize.css und Bootstrap haben aehnliche Listen. Die Liste
/// ist ein `Vec`, gespeichert wird nur, was dasteht — die Grenze schuetzt
/// nur noch vor Missbrauch (M4).
pub const MAX_SELEKTOREN_JE_REGEL: usize = 512;

/// Maximale verschiedene Schluessel je Element (M4, K7c 01.10.2026).
///
/// Der Schluesselbehaelter traegt Eigenschaften, die gelesen, aber
/// nicht typisiert gerechnet werden (CSS Values 3 §3 nennt das den
/// „berechneten Wert" fuer alles, was weder Zahl noch Farbe ist).
///
/// 48 ist eine GEMESSENE Grenze: `examples/abdeckung` zaehlt auf 35
/// echten Stylesheets bis zu 26 verschiedene Schluessel je Element.
/// 48 laesst Luft und ist 2 Byte je Name plus der Text — bei 5000
/// Knoten und 48 Schluesseln sind das rund 700 kB, was die Seite
/// groesser macht als die Seite selbst.
///
/// Ueber der Grenze wird GEZAEHLT, nicht abgeschnitten: die letzten
/// Namen wegzulassen waere eine halbe CSS-Regel.
pub const MAX_SCHLUESSEL: usize = 48;

/// Maximale Kettenglieder je Selektor (`div > p + span` = 3).
/// Begruendung: Verschachtelung tiefer als 64 gibt es nur in Bomben;
/// laengere Selektoren sind ungueltig (Abschneiden wuerde falsch
/// treffen, Wegwerfen trifft nie falsch).
/// Maximale Maskenebenen je `mask-image` (M4, K7 01.10.2026).
///
/// Begruendung: `mask-image` nimmt eine kommagetrennte Liste von
/// Ebenen (CSS Masking 1 §4.1). Auf wikipedia_vector-2022.css
/// GEMESSEN: die haeufigste Stelle hat EINEN Eintrag, die groesste
/// gefundene Liste drei. Vier laesst Luft.
///
/// Mehr faellt GANZ weg, nicht abgeschnitten: eine abgeschnittene
/// Liste verliert die untere Ebene, ohne dass man es am Bild sieht.
pub const MAX_MASKEN: usize = 4;

/// Maximale Schatten je `box-shadow` (M4, K6 01.10.2026).
///
/// Begruendung: `box-shadow` nimmt eine durch Komma getrennte Liste
/// (`text-shadow` drei, `box-shadow` beliebig viele). Acht ist mehr,
/// als auf einer sichtbaren Seite vorkommt — auf github.com stehen
/// sieben. Jeder Eintrag ist 20 Byte, also 160 Byte je Element.
pub const MAX_SCHATTEN: usize = 8;

pub const MAX_SELEKTOR_TEILE: usize = 64;

/// Maximale Deklarationen je Regel und je style-Attribut. Begruendung:
/// mehr als 64 Eigenschaften pro Block schreibt niemand von Hand.
pub const MAX_DEKLARATIONEN_JE_REGEL: usize = 64;

/// Maximale Zeichen je Deklarationswert.
///
/// Begruendung: einzelne Werte (z.B. `font-family`-Listen) bleiben
/// unter 100 Zeichen.
///
/// K1, 01.10.2026 — ERHOEHT von 512 auf 1024, mit einer Rechnung statt
/// mit einer Vermutung: der groesste gueltige Verlauf hat
/// `MAX_VERLAUF_STOPPS` (32) Stopps. Der kuerzeste Stopp ist
/// `rgb(0,0,0) 0%` (13 Zeichen), der laengste in der Praxis etwa
/// `color-mix(in srgb, #ff0000 50%, #0000ff) 99%` (41). Mit
/// Komma und Leer: 32 x 41 + 31 = 1343 — passt nicht in 512. Bei 1024
/// passen immerhin 24 Stopps in der laengsten Form und alle 32 in der
/// kuerzesten.
///
/// WARUM DAS DER SPEICHER BLEIBT (M4): die Zahl begrenzt den
/// ROHTEXT einer Deklaration, nicht die Menge. Der Rohtext kann nur so
/// lang sein wie die Eingabe, und die ist auf [`MAX_EINGABE`] (1 MiB)
/// deckelt. Ein Wert von 1024 Zeichen kostet also mindestens 1024
/// Zeichen QUELTEXT — 1 MiB Eingabe kaufen hoechstens 1024 solche
/// Werte, also 1 MiB Rohtext. Die Grenze kann den Speicher nicht
/// vergroessern, sie begrenzt nur die einzelne Deklaration.
pub const MAX_DEKL_WERT_LAENGE: usize = 1_024;

/// Maximale Klammertiefe beim Ueberspringen kaputter Bloecke.
/// Begruendung: CSS verschachtelt nur @media (Tiefe 1); 32 fängt die
/// C6-Bombe (100.000 Klammern) ab, ohne legitime Bloecke zu treffen.
pub const MAX_KLAMMER_TIEFE: usize = 32;

/// Maximale Ahnenkette beim Pruefen von Nachfahren-Selektoren.
/// Begruendung: HTML begrenzt seine Baumtiefe (MAX_TIEFE dort); 256
/// Ahnen reichen fuer jede echte Seite, danach gilt "kein Treffer".
pub const MAX_AHNEN: usize = 256;

/// Maximale Geschwisterpruefung fuer `~`. Begruendung: Listen mit mehr
/// als 512 Eintraegen davor sind pathologisch; danach gilt "kein
/// Treffer", statt die Liste zu durchlaufen.
pub const MAX_GESCHWISTER_SCAN: usize = 512;

/// Hoechstzahl Kandidaten, die ein RELATIVER Selektor (das Innere von
/// `:has()`) gleichzeitig in der Menge haelt.
///
/// GEMESSENER FEHLER 30.09.2026 (R0 c): `:has()` ist als Rekursion
/// geschrieben worden, mit einer Tiefengrenze von 10 statt mit einer
/// Mengengrenze. Eine Seite mit 50 000 Knoten unter dem Wurzelknoten
/// und `:has(div)` im Autoren-Stylesheet liest damit 50 000 Knoten —
/// fuer JEDEN Knoten, fuer den die Kaskade laeuft. Das ist 50 000 x
/// 50 000. Die Kandidatenmenge ist jetzt auf 2 056 begrenzt und die
/// Gesamtzahl besuchter Knoten auf `MAX_HAS_BESUCHE`; danach gilt
/// "kein Treffer".
pub const MAX_HAS_KANDIDATEN: usize = 2_056;

/// Gesamtbesuche fuer EINEN `:has()`-Vergleich (M3/M4). 4 096 Knoten
/// sind auf einer echten Seite die groesste sinnvolle Tiefe; danach
/// ist "kein Treffer" die ehrliche Antwort, und das Abschneiden wird
/// gemeldet statt zu haengen.
pub const MAX_HAS_BESUCHE: usize = 4_096;

/// Laengste Token-Einzeltext (Ident, Zeichenkette, ...). Begruendung:
/// Schluesselwoerter sind kurz; was laenger als 256 Zeichen ist, kann
/// kein Selektor- oder Eigenschaftsname sein und wird gekuerzt.
pub const MAX_TOKEN_TEXT: usize = 256;

/// Betragsgroesste px-Zahl nach der Aufloesung. Begruendung: kein
/// Ausgabegeraet zeigt 100.000 px; groessere Werte stammen aus
/// Ueberlaeufen oder Bomben (`width: 1e30px`) und werden gekappt.
pub const MAX_PX: f32 = 100_000.0;

/// Maximale Raster-Zellen je Aufruf. Begruendung: Kachelseiten haben
/// hunderte Zellen; 1024 deckt sie ab, die Zeilenschleife bleibt kurz.
pub const MAX_RASTER_ZELLEN: usize = 1024;

/// Maximale Spalten je grid-template-columns. Begruendung: echte Raster
/// haben ein Dutzend Spalten; 32 laesst viel Luft, repeat() kann nicht
/// wuchern (M4).
pub const MAX_RASTER_SPALTEN: usize = 32;

/// Maximale Variablendefinitionen je Regel und je style-Attribut.
///
/// Release 08.10.2026: GEMESSEN schreiben Design-Systeme sehr wohl mehr
/// als 16 je Block — github.com setzt rund 2000 Farb-Variablen in EINER
/// Regel, Tailwind 4 einige Hundert in `:root`. Der Rest faellt weg (M4).
pub const MAX_VAR_JE_REGEL: usize = 4096;

/// Maximale verschiedene Variablen-Gebote je Element (M4).
///
/// Release 08.10.2026: die Vererbung klont nicht mehr (siehe
/// `typen::Variablen`, geteilte Ebenen) — die Grenze schuetzt nur noch
/// vor Bomben, nicht vor echten Design-Systemen.
pub const MAX_VAR_EINTRAEGE: usize = 8192;

/// Maximale Ersetzungstiefe fuer var() (Zyklen Schutz, M5).
/// Begruendung: echte Ketten sind 1-3 tief; tiefer ist ein Zyklus oder
/// eine Bombe (--a: var(--b), --b: var(--a), ...).
pub const MAX_VAR_TIEFE: u8 = 8;

/// Maximale var()-Ersetzungen je Wert. Begruendung: ein Wert hat eine
/// Handvoll var(); mehr ist pathologisch — Rest bleibt unaufgeloest
/// (Deklaration faellt weg).
pub const MAX_VAR_ERSETZUNGEN: usize = 32;

/// Maximale Rekursionstiefe des calc-Parsers (M5). Begruendung: echte
/// Formeln sind flach; tiefer ist eine Bombe ("(((...").
pub const MAX_RECHEN_TIEFE: u8 = 16;

/// Maximale Farbstopps je Verlauf (M3/M4, K1 01.10.2026).
///
/// Begruendung: `MAX_DEKL_WERT_LAENGE` (512 Zeichen) fasst hoechstens
/// eine Hand voll Stopps; CSS Images 3, §3.5 laesst beliebig viele zu.
/// Der Wert ist die Grenze des Speichers, nicht die der Spezifikation:
/// 32 Stopps sind auf einem Bild noch nicht unterscheidbar (bei 1000 px
/// Breite liegt alle 31 px einer). Alles darueber wird abgeschnitten und
/// der Rest des Verlaufs als ungueltig verworfen — nie als "gilt".
pub const MAX_VERLAUF_STOPPS: usize = 32;

/// Maximale Zeichenzahl des Innenteils einer Verlaufsfunktion (M4).
///
/// Begruendung: `MAX_DEKL_WERT_LAENGE` ist die aeussere Grenze, hier
/// steht dieinnere gegen Klammertiefen-Fallen. Die laengste Angabe, die
/// noch sinnvoll ist, hat mit 32 Stopps rund 400 Zeichen; 1024 reicht
/// fuer jede reale Deklaration (inklusive `rgba(...)` je Stopp).
pub const MAX_VERLAUF_INNEN: usize = 1_024;

/// Maximale Argumente je min()/max() und Eintraege je calc-Summe.
/// Begruendung: mehr als 16 Summanden schreibt niemand.
pub const MAX_RECHEN_ARGS: usize = 16;

/// So oft `cubic-bezier()` iteriert, bevor der letzte Wert genommen wird
/// (CSS Easing 1 §3.2: „iterates at most 8 times").
///
/// M3: Eine Newton-Suche ohne Schrittgrenze ist eine Endlosschleife in
/// Gleitkomma. Die Zahl ist die Vorgabe der Spec, nicht meine Wahl;
/// `bewegung.rs` weist nach, dass sie auf 4096 Stuetzstellen einen
/// Restfehler unter 1e-6 laesst.
pub const MAX_NEWTON_SCHRITTE: u32 = 8;

/// NUR FUER DIE MESSUNG (K4, 01.10.2026): wenn `true`, arbeitet die
/// Kaskade OHNE Selektor-Buckets — exakt der Zustand vor K4. Damit
/// ist "mit" gegen "ohne" eine Zahl und keine Behauptung.
///
/// Steht hier und nicht in `kaskade.rs`, weil es die oeffentliche
/// Schaltflaeche des Messprogramms ist. Nach dem Abschluss von K4
/// wird es entfernt.
pub static VORMERKER_AUS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// NUR FUER DIE MESSUNG (K11, 02.10.2026): wenn `true`, arbeitet die
/// Kaskade OHNE die billige Vorpruefung — jede Regel wird dann
/// vollstaendig durch `bieten` geschickt.
///
/// GEMESSENER Anlass: die Phasenmessung in `examples/kaskade_phasen.rs`
/// hat gezeigt, dass die Regelschleife 97,6 % der Kaskadenzeit
/// ausmacht (84.9 von 84.9 us je Knoten, davon 0.543 us fuer die
/// Maske). 350 `bieten` je Knoten sind 0.24 us je Regel. Um zu wissen,
/// WIE VIELE dieser 0.24 us die Vorpruefung kostet und wie viele
/// `bieten` selbst, muss man sie abschalten koennen.
///
/// Steht hier und nicht in `kaskade.rs`, weil es die oeffentliche
/// Schaltflaeche des Messprogramms ist. Standard `false`, also
/// normaler Betrieb mit Vorpruefung.
pub static VORPRUEFUNG_AUS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// K17 (02.10.2026): schaltet den NAMENSGESCHLUESSELTEN Eimer wieder ab
/// und legt die generischen Eigenschaften zurueck in ihren gemeinsamen
/// Kaskaden-Slot — also genau den Zustand, der vor K17 gemessen wurde.
///
/// Grund der Schaltflaeche: der Eimer kostet einen linearen Scan und
/// einen `String`-Vergleich je angebotener generischer Deklaration. Ohne
/// A/B auf DEMSELBEN Binary laesst sich nicht unterscheiden, ob eine
/// Zeitdifferenz von diesem Code kommt oder vom Tagesrauschen.
///
/// Standard `false`, also normaler Betrieb mit dem neuen Eimer.
pub static SCHLUESSEL_EIMER_AUS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Standard-Schriftgroesse in px (rem-Basis, medium, `initial`).
/// Begruendung: Browser-Voreinstellung seit jeher 16 px.
pub const BASIS_SCHRIFT_PX: f32 = 16.0;
